// Shared helpers for the release pipeline scripts in this folder (.github/workflows/release.yml).
// No dependencies: these run on a bare runner, before or without `npm ci`.
import { createHash } from 'node:crypto';
import { appendFileSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** The installers each platform job builds. The one table the plan, the collector and the
 *  assembler agree on. `os` is the GitHub runner, `args` what the Tauri CLI gets. */
export const PLATFORMS = [
  {
    id: 'macos',
    name: 'macOS (universal)',
    os: 'macos-latest',
    args: '--target universal-apple-darwin',
    target: 'universal-apple-darwin',
    rust_targets: 'aarch64-apple-darwin,x86_64-apple-darwin',
  },
  { id: 'windows', name: 'Windows (NSIS installer)', os: 'windows-latest', args: '', target: '', rust_targets: '' },
  { id: 'linux', name: 'Linux (deb + AppImage)', os: 'ubuntu-22.04', args: '', target: '', rust_targets: '' },
];

/** Asset names are used in URLs and on a release page: keep them boring. */
export const SAFE_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

export class Stop extends Error {}

/** True when the module with this `import.meta.url` is the script node was started with
 *  (so the same file can be run by the workflow and imported by the tests). */
export function isMain(metaUrl) {
  if (!process.argv[1]) return false;
  const norm = (p) => {
    try {
      p = realpathSync.native(p);
    } catch {}
    return process.platform === 'win32' ? p.toLowerCase() : p;
  };
  return norm(fileURLToPath(metaUrl)) === norm(process.argv[1]);
}

/** Ends the script with one clear error line (shown as an annotation on the run). */
export function stop(message) {
  throw new Stop(message);
}

/** Runs `main`, turning a Stop into a clean failure and anything else into a stack trace. */
export async function run(main) {
  try {
    await main();
  } catch (e) {
    if (e instanceof Stop) {
      console.error(`::error::${e.message.replace(/\r?\n/g, ' ')}`);
    } else {
      console.error(e);
    }
    process.exit(1);
  }
}

export function setOutput(name, value) {
  const text = String(value);
  if (/[\r\n]/.test(text)) stop(`Output ${name} must be a single line`);
  console.log(`${name}=${text}`);
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${text}\n`);
}

export function summary(markdown) {
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown + '\n');
}

export function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

/** The app version: [workspace.package] in the root Cargo.toml (see scripts/set-version.mjs). */
export function readVersion(root = ROOT) {
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
  const m = cargo.match(/\[workspace\.package\][^\[]*?\nversion\s*=\s*"([^"]*)"/);
  if (!m) stop('Could not find [workspace.package] version in Cargo.toml');
  return m[1];
}

export function megabytes(bytes) {
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A very small GitHub REST client. `fetchImpl` is replaceable so the logic can be tested. */
export class GitHub {
  constructor({ token, repo, api, uploads, fetchImpl, retryDelayMs } = {}) {
    this.token = token ?? process.env.GITHUB_TOKEN ?? '';
    this.repo = repo ?? process.env.GITHUB_REPOSITORY ?? '';
    this.api = (api ?? process.env.GITHUB_API_URL ?? 'https://api.github.com').replace(/\/$/, '');
    this.uploads = (uploads ?? process.env.GITHUB_UPLOADS_URL ?? 'https://uploads.github.com').replace(/\/$/, '');
    this.fetch = fetchImpl ?? globalThis.fetch;
    this.retryDelayMs = retryDelayMs ?? 2000;
    if (!this.repo) stop('GITHUB_REPOSITORY is not set');
  }

  headers(extra = {}) {
    return {
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
      'User-Agent': 'thoughts-release-pipeline',
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
      ...extra,
    };
  }

  /**
   * One request. Returns { status, data }. Statuses listed in `allow` are returned to the caller;
   * any other 4xx stops the script; 5xx and network errors are retried a few times.
   */
  async request(method, path, { body, allow = [], base, headers, timeoutMs = 30000, attempts = 3 } = {}) {
    const url = path.startsWith('http') ? path : `${base ?? this.api}${path}`;
    let last = '';
    for (let attempt = 1; attempt <= attempts; attempt++) {
      try {
        const init = { method, headers: this.headers(headers), signal: AbortSignal.timeout(timeoutMs) };
        if (body !== undefined) {
          if (Buffer.isBuffer(body)) {
            init.body = body;
          } else {
            init.body = JSON.stringify(body);
            init.headers['Content-Type'] = 'application/json';
          }
        }
        const res = await this.fetch(url, init);
        const text = await res.text();
        let data = null;
        if (text) {
          try {
            data = JSON.parse(text);
          } catch {
            data = text;
          }
        }
        if (res.ok || allow.includes(res.status)) return { status: res.status, data };
        last = `${res.status} ${typeof data === 'string' ? data.slice(0, 300) : JSON.stringify(data).slice(0, 300)}`;
        if (res.status < 500 && res.status !== 429) break;
      } catch (e) {
        last = String(e?.message ?? e);
      }
      if (attempt < attempts) await sleep(this.retryDelayMs * attempt);
    }
    stop(`GitHub API ${method} ${url.replace(/\?.*$/, '')} failed: ${last}`);
  }

  /** Every item of a paged list endpoint that returns an array. */
  async list(path) {
    const all = [];
    for (let page = 1; page <= 50; page++) {
      const sep = path.includes('?') ? '&' : '?';
      const { data } = await this.request('GET', `${path}${sep}per_page=100&page=${page}`);
      if (!Array.isArray(data)) stop(`GitHub API GET ${path} did not return a list`);
      all.push(...data);
      if (data.length < 100) break;
    }
    return all;
  }
}

export { sleep };
