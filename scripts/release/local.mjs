#!/usr/bin/env node
// A release made on this Mac (macOS only), for as long as the pipeline cannot run. It does what
// .github/workflows/release.yml does, with the same scripts, and nothing it builds leaves this
// computer until it is asked to:
//
//   node scripts/release/local.mjs             build, sign, assemble and check. Nothing is sent.
//   node scripts/release/local.mjs --draft     the same, then push `main` and the tag (with the
//                                              GitHub workflows switched off meanwhile) and attach
//                                              the files to a DRAFT release. Nobody is offered a draft.
//   node scripts/release/local.mjs --publish   make that draft public. This is the moment the
//                                              update reaches every installed copy.
//
//   --skip-tests   do not run the tests first (the summary says so)
//
// Before it: set the version (scripts/set-version.mjs), write the entry in docs/CHANGELOG.md and
// docs/ar/CHANGELOG.md (it becomes the update dialog's text and the release page), commit.
//
// It needs:
//   - the updater's private key in ~/.tauri/thoughts-updater.key, or TAURI_SIGNING_PRIVATE_KEY
//     holding the key or the path of its file;
//   - the key's password in TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or in the macOS keychain:
//       security add-generic-password -s thoughts-updater -a release -w
//   - the public key committed as src-tauri/updater.pub;
//   - `gh` signed in, for --draft and --publish.
//
// The key is handed to one step only, the one that packs and signs; the compile step, where the
// build code of every dependency runs, never sees it (scripts/tauri-ci.mjs, as in the pipeline).
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, unlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { assemble, changelogSection, notesFor } from './assemble.mjs';
import { collect } from './collect.mjs';
import { GitHub, isMain, megabytes, PLATFORMS, readVersion, ROOT, run, stop } from './lib.mjs';
import { checkSignature } from './minisign.mjs';
import { publish, refusePublished } from './publish.mjs';

const MAC = PLATFORMS.find((p) => p.id === 'macos');
export const DEFAULT_KEY = join(homedir(), '.tauri', 'thoughts-updater.key');
const PUBLIC_KEY = join(ROOT, 'src-tauri', 'updater.pub');
const KEYCHAIN_SERVICE = 'thoughts-updater';
/** The workflows a push to `main` or of a tag would start. */
const WORKFLOWS = ['CI', 'Release', 'Advisories'];

/** `owner/name` from the address of the `origin` remote. */
export function repoFromRemote(url) {
  const m = String(url).trim().match(/github\.com[:/]([\w.-]+)\/([\w.-]+?)(?:\.git)?\/?$/);
  return m ? `${m[1]}/${m[2]}` : null;
}

/** The address every installed copy asks first. Public repositories answer it; a private one
 *  answers "not found", and a copy with the GitHub fallback switched on goes on from there. */
export const publicEndpoint = (repo) => `https://github.com/${repo}/releases/latest/download/latest.json`;

/** The private key: what TAURI_SIGNING_PRIVATE_KEY holds (the key, or the path of its file), or
 *  the file at the usual place. Null when there is none. */
export function findKey(env, { exists = existsSync, read = (p) => readFileSync(p, 'utf8'), fallback = DEFAULT_KEY } = {}) {
  const given = (env.TAURI_SIGNING_PRIVATE_KEY || '').trim();
  if (given) return exists(given) ? read(given).trim() : given;
  return exists(fallback) ? read(fallback).trim() : null;
}

/** Everything that must hold before a release is built. Returns the reasons it does not. */
export function preflightProblems({ branch, dirty, version, head, tagAt, key, password, publicKey, keysMatch, changelog, changelogAr }) {
  const problems = [];
  if (branch !== 'main') problems.push(`Releases are made from main; this is "${branch}".`);
  if (dirty) problems.push('There are changes that are not committed. A release is built from a commit.');
  if (!/^\d+\.\d+\.\d+$/.test(version || '')) problems.push(`"${version}" is not a version like 1.2.3.`);
  if (tagAt && tagAt !== head) problems.push(`The tag v${version} already exists on another commit (${tagAt.slice(0, 7)}). Set a new version, or delete the tag if that version was never published.`);
  if (!key) problems.push(`The updater's private key was not found (looked in TAURI_SIGNING_PRIVATE_KEY and ${DEFAULT_KEY}).`);
  if (key && password === null) problems.push(`The key's password was not found. Set TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or save it in the keychain: security add-generic-password -s ${KEYCHAIN_SERVICE} -a release -w`);
  if (!publicKey) problems.push('src-tauri/updater.pub is missing. Commit the public key (the .pub file made with the private key): it is not secret, and every build reads it from there.');
  if (key && publicKey && password !== null && keysMatch === false) problems.push('The private key does not match src-tauri/updater.pub (or its password is wrong). A copy built with the wrong public key can never be updated.');
  if (!changelog) problems.push(`docs/CHANGELOG.md has no entry "## ${version}". It becomes the text of the update dialog and of the release page.`);
  if (!changelogAr) problems.push(`docs/ar/CHANGELOG.md has no entry "## ${version}". Copies set to Arabic show it in the update dialog.`);
  return problems;
}

/** Of the workflows a push starts, the ones that are switched on now (and so must be switched
 *  off for the push, and on again afterwards). `listed` is `gh workflow list --all --json name,state,id`. */
export function workflowsToSwitch(listed) {
  return (Array.isArray(listed) ? listed : []).filter((w) => WORKFLOWS.includes(w.name) && w.state === 'active').map((w) => ({ id: String(w.id), name: w.name }));
}

// ------------------------------------------------------------------ the parts that touch things

function sh(cmd, args, { env = process.env, capture = false, allowFail = false, input } = {}) {
  const r = spawnSync(cmd, args, { cwd: ROOT, env, input, stdio: capture ? ['pipe', 'pipe', 'pipe'] : 'inherit', encoding: 'utf8' });
  if (r.error) stop(`Could not run ${cmd}: ${r.error.message}`);
  if (r.status !== 0 && !allowFail) stop(`${cmd} ${args.slice(0, 3).join(' ')} failed${capture ? `: ${(r.stderr || r.stdout || '').trim().slice(0, 400)}` : ''}`);
  return { ok: r.status === 0, out: (r.stdout || '').trim() };
}

const git = (...args) => sh('git', args, { capture: true, allowFail: true });
const node = (script, args = [], env = process.env) => sh(process.execPath, [join(ROOT, script), ...args], { env });

function findPassword(env) {
  if (env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD !== undefined) return env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
  if (process.platform !== 'darwin') return null;
  const r = sh('security', ['find-generic-password', '-s', KEYCHAIN_SERVICE, '-w'], { capture: true, allowFail: true });
  return r.ok ? r.out : null;
}

/** Does `key` (with `password`) sign what `publicKey` verifies? Signs a few bytes and checks. */
function keysAgree(key, password, publicKey) {
  const dir = mkdtempSync(join(tmpdir(), 'thoughts-keycheck-'));
  try {
    const file = join(dir, 'check.txt');
    writeFileSync(file, 'thoughts updater key check\n');
    const cli = join(ROOT, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
    const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password };
    const r = sh(process.execPath, [cli, 'signer', 'sign', file], { env, capture: true, allowFail: true });
    if (!r.ok || !existsSync(`${file}.sig`)) return false;
    return checkSignature({ publicKeyFile: publicKey, signatureFile: readFileSync(`${file}.sig`, 'utf8'), data: readFileSync(file) }).ok;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function facts() {
  const version = readVersion();
  const changelog = (file) => (existsSync(join(ROOT, file)) ? readFileSync(join(ROOT, file), 'utf8') : '');
  const key = findKey(process.env);
  const password = key ? findPassword(process.env) : null;
  const publicKey = existsSync(PUBLIC_KEY) ? readFileSync(PUBLIC_KEY, 'utf8').trim() : '';
  const tag = git('rev-parse', '-q', '--verify', `refs/tags/v${version}^{commit}`);
  return {
    version,
    tag: `v${version}`,
    branch: git('branch', '--show-current').out,
    dirty: git('status', '--porcelain').out !== '',
    head: git('rev-parse', 'HEAD').out,
    tagAt: tag.ok ? tag.out : null,
    repo: repoFromRemote(git('remote', 'get-url', 'origin').out),
    key,
    password,
    publicKey,
    keysMatch: key && publicKey && password !== null ? keysAgree(key, password, publicKey) : null,
    changelog: changelogSection(version, changelog('docs/CHANGELOG.md')),
    changelogAr: changelogSection(version, changelog('docs/ar/CHANGELOG.md')),
    notes: notesFor(version, changelog('docs/CHANGELOG.md')),
    notesAr: notesFor(version, changelog('docs/ar/CHANGELOG.md')),
  };
}

/** Installers of an earlier version left in the bundle folders would be picked up as this one's. */
function clearOldInstallers(bundle) {
  for (const [dir, endings] of [['dmg', ['.dmg']], ['macos', ['.app.tar.gz', '.app.tar.gz.sig']]]) {
    const folder = join(bundle, dir);
    if (!existsSync(folder)) continue;
    for (const e of readdirSync(folder, { withFileTypes: true })) {
      if (e.isFile() && endings.some((x) => e.name.endsWith(x))) unlinkSync(join(folder, e.name));
    }
  }
}

function build(f) {
  const out = join(ROOT, 'release-out');
  const meta = join(ROOT, 'release-meta');
  for (const dir of [out, meta]) rmSync(dir, { recursive: true, force: true });
  clearOldInstallers(join(ROOT, 'target', MAC.target, 'release', 'bundle'));

  const conf = ['--config', 'src-tauri/release.conf.json', ...MAC.args.split(' ')];
  node('scripts/release-config.mjs', [], { ...process.env, UPDATER_PUBKEY: '', UPDATER_ENDPOINT: publicEndpoint(f.repo) });
  // Compile: no signing material in the environment (tauri-ci.mjs removes whatever is there).
  node('scripts/tauri-ci.mjs', ['build', '--no-bundle', ...conf]);
  // Pack and sign: the one step that sees the key.
  node('scripts/tauri-ci.mjs', ['bundle', ...conf], { ...process.env, TAURI_SIGNING_PRIVATE_KEY: f.key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: f.password });

  collect({ platform: 'macos', version: f.version, target: MAC.target, updater: true, out, commit: f.head, signed: { macos: false, notarized: false } });
  const release = assemble({
    assetsDir: out, metaDir: meta, mode: 'release', partial: true, version: f.version, tag: f.tag, platforms: ['macos'],
    repo: f.repo, commit: f.head, notes: f.notes, notesAr: f.notesAr,
  });

  checkSigned(f, out, release);
  return { out, meta, release };
}

/** Check the result the way an installed copy will: every update file must be signed with the
 *  committed key, for exactly this version. */
function checkSigned(f, out, release) {
  const signed = release.files.filter((x) => x.signature);
  if (!signed.length) stop('The build produced no signed update file.');
  for (const file of signed) {
    const said = checkSignature({ publicKeyFile: f.publicKey, signatureFile: readFileSync(join(out, file.signature), 'utf8'), data: readFileSync(join(out, file.name)) });
    if (!said.ok) stop(`${file.name}: ${said.why}. No installed copy would accept it.`);
    if (said.version !== f.version) {
      stop(`${file.name} was signed ${said.version ? `for version ${said.version}` : 'without a version'}, not for ${f.version}. Installed copies refuse such an update (requireSignedVersion).`);
    }
  }
}

/** The files an earlier run of this command built for exactly this commit and version, if they
 *  are still here: `--draft` then sends those instead of building again. (Each file is checked
 *  against its recorded checksum once more before it is uploaded, in publish.mjs.) */
function builtEarlier(f) {
  const out = join(ROOT, 'release-out');
  const meta = join(ROOT, 'release-meta');
  try {
    const release = JSON.parse(readFileSync(join(meta, 'release.json'), 'utf8'));
    if (release.mode !== 'release' || release.tag !== f.tag || release.commit !== f.head || release.updater !== true) return null;
    if (!release.files.every((x) => existsSync(join(x.name === 'latest.json' ? meta : out, x.name)))) return null;
    checkSigned(f, out, release);
    return { out, meta, release };
  } catch {
    return null;
  }
}

function ghToken() {
  const r = sh('gh', ['auth', 'token', '--hostname', 'github.com'], { capture: true, allowFail: true });
  if (!r.ok || !r.out) stop('gh is not signed in. Run: gh auth login');
  return r.out;
}

/** Run `work` with the workflows a push would start switched off, and switch them on again
 *  afterwards whatever happened. */
async function withWorkflowsOff(repo, work) {
  const listed = sh('gh', ['workflow', 'list', '--all', '--json', 'name,state,id', '-R', repo], { capture: true });
  const off = workflowsToSwitch(JSON.parse(listed.out || '[]'));
  for (const w of off) sh('gh', ['workflow', 'disable', w.id, '-R', repo], { capture: true });
  if (off.length) console.log(`Switched off for the push: ${off.map((w) => w.name).join(', ')}.`);
  try {
    return await work();
  } finally {
    for (const w of off) {
      const back = sh('gh', ['workflow', 'enable', w.id, '-R', repo], { capture: true, allowFail: true });
      if (!back.ok) console.error(`::error::Could not switch the ${w.name} workflow back on. Do it by hand: gh workflow enable ${w.id} -R ${repo}`);
    }
    if (off.length) console.log(`Switched back on: ${off.map((w) => w.name).join(', ')}.`);
  }
}

async function draft(f, built) {
  const gh = new GitHub({ token: ghToken(), repo: f.repo });
  await refusePublished({ gh, tag: f.tag });
  return withWorkflowsOff(f.repo, async () => {
    sh('git', ['push', 'origin', 'main']);
    if (!f.tagAt) sh('git', ['tag', f.tag, f.head]);
    sh('git', ['push', 'origin', f.tag]);
    return publish({ gh, tag: f.tag, commit: f.head, assetsDir: built.out, metaDir: built.meta, notes: f.changelog });
  });
}

async function makePublic(f) {
  const gh = new GitHub({ token: ghToken(), repo: f.repo });
  const drafts = (await gh.list(`/repos/${gh.repo}/releases`)).filter((r) => r.tag_name === f.tag);
  if (drafts.some((r) => !r.draft)) stop(`Release ${f.tag} is already public.`);
  if (drafts.length !== 1) stop(`Expected one draft release for ${f.tag}, found ${drafts.length}. Run with --draft first.`);
  const names = (await gh.list(`/repos/${gh.repo}/releases/${drafts[0].id}/assets`)).filter((a) => a.state === 'uploaded').map((a) => a.name);
  for (const need of ['latest.json', '.app.tar.gz', '.app.tar.gz.sig', '.dmg']) {
    if (!names.some((n) => n.endsWith(need))) stop(`The draft for ${f.tag} has no ${need} file. Run with --draft again.`);
  }
  // The tag is on GitHub already (--draft pushed it), so making the release public creates
  // nothing and starts no workflow.
  const { data } = await gh.request('PATCH', `/repos/${gh.repo}/releases/${drafts[0].id}`, { body: { draft: false, make_latest: 'true' } });
  if (data?.draft !== false) stop(`GitHub did not make ${f.tag} public.`);
  return data.html_url;
}

if (isMain(import.meta.url)) {
  await run(async () => {
    const args = new Set(process.argv.slice(2));
    const unknown = [...args].filter((a) => !['--draft', '--publish', '--skip-tests'].includes(a));
    if (unknown.length || (args.has('--draft') && args.has('--publish'))) stop('usage: local.mjs [--draft | --publish] [--skip-tests]');
    if (process.platform !== 'darwin') stop('This command builds the macOS release, so it runs on a Mac.');

    const f = facts();
    if (!f.repo) stop('The origin remote is not a GitHub repository.');

    if (args.has('--publish')) {
      const url = await makePublic(f);
      console.log(`\n${f.tag} is public: ${url}\nEvery copy that checks for updates is offered it from now on.`);
      return;
    }

    const problems = preflightProblems(f);
    if (problems.length) {
      console.error(`Not ready to release ${f.tag}:\n- ${problems.join('\n- ')}\n`);
      stop(`${problems.length === 1 ? 'One thing' : `${problems.length} things`} to put right before ${f.tag} can be released (listed above)`);
    }

    let built = args.has('--draft') ? builtEarlier(f) : null;
    if (built) {
      console.log(`Using the files this command built for ${f.tag} (${f.head.slice(0, 7)}) earlier.`);
    } else {
      if (!args.has('--skip-tests')) {
        sh('cargo', ['test', '-p', 'thoughtsd']);
        sh('cargo', ['test', '-p', 'thoughts']);
        sh('npm', ['run', 'check']);
        sh(process.execPath, ['--test', join(ROOT, 'scripts', 'release', 'release.test.mjs')]);
      }
      built = build(f);
    }
    console.log(`\nBuilt and checked ${f.tag} (${f.head.slice(0, 7)})${args.has('--skip-tests') ? ' — THE TESTS WERE SKIPPED' : ''}:`);
    for (const file of built.release.files) console.log(`  ${file.name}  ${megabytes(file.bytes)}`);
    console.log('  Every signed file verifies against src-tauri/updater.pub, for this version.');

    if (!args.has('--draft')) {
      console.log(`\nNothing was sent anywhere. The files are in release-out/ and release-meta/.\nNext: node scripts/release/local.mjs --draft`);
      return;
    }
    const result = await draft(f, built);
    console.log(`\nDraft release ${f.tag} holds ${result.files.length} files: ${result.url}\nNobody is offered a draft. Try the .dmg, then: node scripts/release/local.mjs --publish`);
  });
}
