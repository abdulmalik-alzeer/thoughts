// Tests for the release pipeline's own scripts. The workflows cannot be run on a developer's
// machine, so everything they decide is in plain functions that can:
//
//   node --test scripts/release/release.test.mjs        (CI runs this in the "UI check" job)
//
// GitHub is replaced by FakeGitHub, a tiny in-memory stand-in that answers the handful of REST
// calls the scripts make, with the rules that matter here (a read-only token cannot see drafts,
// a draft cannot be fetched by tag, a file name can only be uploaded once).
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { releaseConfig } from '../release-config.mjs';
import { NO_HOOKS, prepare, RUNNER_CHANNELS } from '../tauri-ci.mjs';
import { assemble, changelogSection, notesFor, updaterManifest } from './assemble.mjs';
import { decide, verdict } from './ci-status.mjs';
import { collect } from './collect.mjs';
import { GitHub, PLATFORMS, ROOT, Stop } from './lib.mjs';
import { ALWAYS_ASK_ACCOUNT, commitsByOthers, findKey, findPassword, keychainPath, KEYCHAIN_SERVICE, preflightProblems, publicEndpoint, repoFromRemote, TESTS, workflowsToSwitch } from './local.mjs';
import { executable, MANIFEST, pack, place } from './compiled.mjs';
import { checkSignature } from './minisign.mjs';
import { plan } from './plan.mjs';
import { publish, refusePublished } from './publish.mjs';

const REPO = 'someone/thoughts';
const SHA = 'a'.repeat(40);
const tmp = () => mkdtempSync(join(tmpdir(), 'thoughts-release-test-'));
const stops = (pattern) => (e) => e instanceof Stop && pattern.test(e.message);

// ---------------------------------------------------------------------------------- plan.mjs

test('plan: a tag push is a release of every platform', () => {
  const p = plan({ eventName: 'push', refType: 'tag', refName: 'v1.2.3', choice: '', version: '1.2.3' });
  assert.equal(p.mode, 'release');
  assert.equal(p.tag, 'v1.2.3');
  assert.equal(p.platforms, 'macos,windows,linux');
  assert.deepEqual(JSON.parse(p.matrix).map((m) => m.os), ['macos-latest', 'windows-latest', 'ubuntu-22.04']);
  assert.equal(JSON.parse(p.matrix)[0].args, '--target universal-apple-darwin');
});

test('plan: a tag that does not match Cargo.toml stops the run', () => {
  assert.throws(() => plan({ eventName: 'push', refType: 'tag', refName: 'v1.2.4', version: '1.2.3' }), stops(/does not match/));
  assert.throws(() => plan({ eventName: 'push', refType: 'tag', refName: '1.2.3', version: '1.2.3' }), stops(/does not match/));
});

test('plan: a manual run is a rehearsal, whatever ref it runs on, and can build one platform', () => {
  for (const refType of ['branch', 'tag']) {
    const p = plan({ eventName: 'workflow_dispatch', refType, refName: 'v9.9.9', choice: 'all', version: '1.2.3' });
    assert.equal(p.mode, 'rehearsal');
    assert.equal(p.platforms, 'macos,windows,linux');
    assert.equal(p.tag, 'v1.2.3');
  }
  const one = plan({ eventName: 'workflow_dispatch', refType: 'branch', refName: 'main', choice: 'windows', version: '1.2.3' });
  assert.equal(one.platforms, 'windows');
  assert.throws(() => plan({ eventName: 'workflow_dispatch', refType: 'branch', refName: 'main', choice: 'beos', version: '1.2.3' }), stops(/Unknown platform/));
});

test('plan: a branch push or the schedule only warms the Windows and Linux caches', () => {
  for (const eventName of ['push', 'schedule']) {
    const p = plan({ eventName, refType: 'branch', refName: 'main', choice: '', version: '1.2.3' });
    assert.equal(p.mode, 'warm');
    assert.equal(p.platforms, 'windows,linux');
  }
});

test('plan: anything else is refused', () => {
  assert.throws(() => plan({ eventName: 'pull_request', refType: 'branch', refName: 'x', version: '1.2.3' }), stops(/does not know/));
});

// ----------------------------------------------------------------------------- ci-status.mjs

const ciRun = (status, conclusion) => ({ status, conclusion, html_url: 'https://example.invalid/run' });

test('ci verdict', () => {
  assert.equal(verdict([]), 'none');
  assert.equal(verdict([ciRun('completed', 'failure'), ciRun('completed', 'cancelled')]), 'none');
  assert.equal(verdict([ciRun('in_progress', null)]), 'pending');
  assert.equal(verdict([ciRun('queued', null), ciRun('completed', 'failure')]), 'pending');
  assert.equal(verdict([ciRun('completed', 'failure'), ciRun('completed', 'success')]), 'passed');
});

/** A GitHub whose CI answers come from a script: one list of runs per poll. */
function ciGitHub(polls, branchHead = SHA) {
  let i = 0;
  const gh = new GitHub({ token: 't', repo: REPO, fetchImpl: async () => { throw new Error('unused'); } });
  gh.request = async (_method, path) => {
    if (path.includes('/branches/')) return { status: 200, data: { commit: { sha: branchHead } } };
    return { status: 200, data: { workflow_runs: polls[Math.min(i++, polls.length - 1)] } };
  };
  return gh;
}
const quiet = { pause: async () => {}, log: () => {} };

test('ci-status: a commit that already passed CI is not tested again', async () => {
  const r = await decide({ gh: ciGitHub([[ciRun('completed', 'success')]]), sha: SHA, defaultBranch: 'main', ...quiet });
  assert.equal(r.needed, false);
});

test('ci-status: waits for a run that is still going, then follows its verdict', async () => {
  const passed = await decide({ gh: ciGitHub([[ciRun('in_progress', null)], [ciRun('in_progress', null)], [ciRun('completed', 'success')]]), sha: SHA, defaultBranch: 'main', ...quiet });
  assert.equal(passed.needed, false);
  const failed = await decide({ gh: ciGitHub([[ciRun('queued', null)], [ciRun('completed', 'failure')]]), sha: SHA, defaultBranch: 'main', ...quiet });
  assert.equal(failed.needed, true);
});

test('ci-status: failed, cancelled or missing CI means the release runs the tests itself', async () => {
  assert.equal((await decide({ gh: ciGitHub([[ciRun('completed', 'cancelled')]]), sha: SHA, defaultBranch: 'main', ...quiet })).needed, true);
  // Not the head of main and no run: nothing to wait for.
  assert.equal((await decide({ gh: ciGitHub([[]], 'b'.repeat(40)), sha: SHA, defaultBranch: 'main', ...quiet })).needed, true);
});

test('ci-status: tag and branch pushed together — waits for the run to appear', async () => {
  const r = await decide({ gh: ciGitHub([[], [], [ciRun('queued', null)], [ciRun('completed', 'success')]]), sha: SHA, defaultBranch: 'main', ...quiet });
  assert.equal(r.needed, false);
});

test('ci-status: gives up waiting and runs the tests rather than block for ever', async () => {
  let clock = 0;
  const r = await decide({ gh: ciGitHub([[ciRun('in_progress', null)]]), sha: SHA, defaultBranch: 'main', now: () => (clock += 60_000), waitMs: 5 * 60_000, ...quiet });
  assert.equal(r.needed, true);
  let clock2 = 0;
  const none = await decide({ gh: ciGitHub([[]]), sha: SHA, defaultBranch: 'main', now: () => (clock2 += 30_000), ...quiet });
  assert.equal(none.needed, true);
});

// ------------------------------------------------------------------- collect.mjs + assemble.mjs

/** A fake checkout with the bundle folders a platform's `tauri bundle` leaves behind. */
function builtTree({ platform, version = '1.2.3', updater = false, omit = [] }) {
  const root = tmp();
  mkdirSync(join(root, 'src-tauri'), { recursive: true });
  writeFileSync(join(root, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ productName: 'Thoughts' }));
  const put = (dir, name, content = `${platform}:${name}`) => {
    if (omit.includes(name)) return;
    mkdirSync(join(root, dir), { recursive: true });
    writeFileSync(join(root, dir, name), content);
  };
  if (platform === 'macos') {
    const b = 'target/universal-apple-darwin/release/bundle';
    put(`${b}/dmg`, `Thoughts_${version}_universal.dmg`);
    put(`${b}/macos/Thoughts.app/Contents`, 'Info.plist');
    if (updater) {
      put(`${b}/macos`, 'Thoughts.app.tar.gz');
      put(`${b}/macos`, 'Thoughts.app.tar.gz.sig', 'SIG-MAC');
    }
  } else if (platform === 'windows') {
    put('target/release/bundle/nsis', `Thoughts_${version}_x64-setup.exe`);
    if (updater) put('target/release/bundle/nsis', `Thoughts_${version}_x64-setup.exe.sig`, 'SIG-WIN\n');
  } else {
    put('target/release/bundle/deb', `Thoughts_${version}_amd64.deb`);
    put(`target/release/bundle/deb/Thoughts_${version}_amd64/data`, 'leftover');
    put('target/release/bundle/appimage', `Thoughts_${version}_amd64.AppImage`);
    put('target/release/bundle/appimage/Thoughts.AppDir', 'AppRun');
    if (updater) {
      put('target/release/bundle/deb', `Thoughts_${version}_amd64.deb.sig`, 'SIG-DEB');
      put('target/release/bundle/appimage', `Thoughts_${version}_amd64.AppImage.sig`, 'SIG-APPIMAGE');
    }
  }
  return root;
}

const fakeTar = (dir, archive) => writeFileSync(join(dir, archive), 'packed app');

/** Runs collect for the given platforms into one folder, like the merged workflow artifacts. */
function collected({ platforms = ['macos', 'windows', 'linux'], version = '1.2.3', updater = false, commit = SHA } = {}) {
  const out = tmp();
  for (const platform of platforms) {
    const target = PLATFORMS.find((p) => p.id === platform).target;
    collect({ platform, version, target, updater, root: builtTree({ platform, version, updater }), out, commit, tar: fakeTar, signed: {} });
  }
  return out;
}

const assembleArgs = (assetsDir, extra = {}) => ({
  assetsDir,
  metaDir: tmp(),
  mode: 'release',
  version: '1.2.3',
  tag: 'v1.2.3',
  platforms: ['macos', 'windows', 'linux'],
  repo: REPO,
  commit: SHA,
  ...extra,
});

test('collect: an unsigned build yields the same five files a 0.0.8 release had', () => {
  const out = collected();
  const release = assemble(assembleArgs(out));
  assert.deepEqual(release.files.map((f) => f.name), [
    'Thoughts_1.2.3_amd64.AppImage',
    'Thoughts_1.2.3_amd64.deb',
    'Thoughts_1.2.3_universal.app.tar.gz',
    'Thoughts_1.2.3_universal.dmg',
    'Thoughts_1.2.3_x64-setup.exe',
  ]);
  assert.equal(release.updater, false);
});

test('collect: stops when an installer is missing, duplicated, or carries another version', () => {
  const args = (platform, root) => ({ platform, version: '1.2.3', target: '', root, out: tmp(), tar: fakeTar });
  assert.throws(() => collect(args('linux', builtTree({ platform: 'linux', omit: ['Thoughts_1.2.3_amd64.AppImage'] }))), stops(/exactly one AppImage/));
  assert.throws(() => collect(args('windows', builtTree({ platform: 'windows', version: '1.2.2' }))), stops(/does not carry version 1.2.3/));
  const two = builtTree({ platform: 'windows' });
  writeFileSync(join(two, 'target/release/bundle/nsis/Thoughts_1.2.2_x64-setup.exe'), 'stale');
  assert.throws(() => collect(args('windows', two)), stops(/exactly one Windows installer/));
  const noApp = builtTree({ platform: 'macos', omit: ['Info.plist'] });
  assert.throws(() => collect({ ...args('macos', noApp), target: 'universal-apple-darwin' }), stops(/Thoughts\.app was not produced/));
});

test('collect: with the updater on, a missing signature stops the build', () => {
  const root = builtTree({ platform: 'linux', updater: true, omit: ['Thoughts_1.2.3_amd64.deb.sig'] });
  assert.throws(() => collect({ platform: 'linux', version: '1.2.3', updater: true, root, out: tmp() }), stops(/amd64\.deb\.sig was not produced/));
  const mac = builtTree({ platform: 'macos', updater: false });
  assert.throws(() => collect({ platform: 'macos', version: '1.2.3', target: 'universal-apple-darwin', updater: true, root: mac, out: tmp(), tar: fakeTar }), stops(/Thoughts\.app\.tar\.gz was not produced/));
});

test('assemble: with the updater on, latest.json covers every platform in one file', () => {
  const out = collected({ updater: true });
  const metaDir = tmp();
  const release = assemble(assembleArgs(out, { metaDir, date: new Date('2026-01-02T03:04:05Z') }));
  assert.equal(release.updater, true);
  const latest = JSON.parse(readFileSync(join(metaDir, 'latest.json'), 'utf8'));
  assert.equal(latest.version, '1.2.3');
  assert.equal(latest.pub_date, '2026-01-02T03:04:05.000Z');
  const url = (name) => `https://github.com/${REPO}/releases/download/v1.2.3/${name}`;
  const mac = { signature: 'SIG-MAC', url: url('Thoughts_1.2.3_universal.app.tar.gz') };
  const win = { signature: 'SIG-WIN', url: url('Thoughts_1.2.3_x64-setup.exe') };
  const appimage = { signature: 'SIG-APPIMAGE', url: url('Thoughts_1.2.3_amd64.AppImage') };
  const deb = { signature: 'SIG-DEB', url: url('Thoughts_1.2.3_amd64.deb') };
  assert.deepEqual(latest.platforms, {
    'darwin-aarch64': mac,
    'darwin-aarch64-app': mac,
    'darwin-x86_64': mac,
    'darwin-x86_64-app': mac,
    'darwin-universal': mac,
    'darwin-universal-app': mac,
    'windows-x86_64': win,
    'windows-x86_64-nsis': win,
    // A copy installed from the .deb is only ever offered the .deb; everything else the AppImage.
    'linux-x86_64': appimage,
    'linux-x86_64-appimage': appimage,
    'linux-x86_64-deb': deb,
  });
  // The signatures and latest.json are release files too, and all of it is in SHA256SUMS.
  const names = release.files.map((f) => f.name);
  for (const n of ['latest.json', 'Thoughts_1.2.3_universal.app.tar.gz.sig', 'Thoughts_1.2.3_x64-setup.exe.sig', 'Thoughts_1.2.3_amd64.deb.sig', 'Thoughts_1.2.3_amd64.AppImage.sig']) {
    assert.ok(names.includes(n), n);
  }
  assert.ok(!names.includes('Thoughts_1.2.3_universal.dmg.sig'));
  assert.equal(readFileSync(join(metaDir, 'SHA256SUMS'), 'utf8').trim().split('\n').length, release.files.length);
});

test('assemble: without the updater there is no latest.json', () => {
  const metaDir = tmp();
  assemble(assembleArgs(collected(), { metaDir }));
  assert.throws(() => readFileSync(join(metaDir, 'latest.json')));
});

test('assemble: a release with a platform missing is refused; a rehearsal of one platform is fine', () => {
  const two = collected({ platforms: ['macos', 'linux'] });
  assert.throws(() => assemble(assembleArgs(two)), stops(/windows build did not deliver/));
  assert.throws(() => assemble(assembleArgs(two, { platforms: ['macos', 'linux'] })), stops(/needs every platform/));
  const one = assemble(assembleArgs(collected({ platforms: ['windows'] }), { mode: 'rehearsal', platforms: ['windows'] }));
  assert.deepEqual(one.files.map((f) => f.name), ['Thoughts_1.2.3_x64-setup.exe']);
});

test('assemble: refuses tampered, stray, wrong-version and wrong-commit files', () => {
  const tampered = collected();
  writeFileSync(join(tampered, 'Thoughts_1.2.3_x64-setup.exe'), 'windows:Thoughts_1.2.3_x64-setup.EXE'); // same size, other content
  assert.throws(() => assemble(assembleArgs(tampered)), stops(/content differs/));

  const stray = collected();
  writeFileSync(join(stray, 'extra.exe'), 'x');
  assert.throws(() => assemble(assembleArgs(stray)), stops(/Unexpected file "extra\.exe"/));

  assert.throws(() => assemble(assembleArgs(collected({ version: '1.2.2' }))), stops(/built version 1\.2\.2/));
  assert.throws(() => assemble(assembleArgs(collected({ commit: 'c'.repeat(40) }))), stops(/built from commit/));

  const mixed = collected({ platforms: ['macos', 'windows'] });
  collect({ platform: 'linux', version: '1.2.3', updater: true, root: builtTree({ platform: 'linux', updater: true }), out: mixed, commit: SHA });
  assert.throws(() => assemble(assembleArgs(mixed)), stops(/on for some platforms and off for others/));
});

test('updater manifest: a single-processor macOS build only claims its own processor', () => {
  const m = updaterManifest({
    infos: [{ platform: 'macos', files: [{ name: 'T.app.tar.gz', kind: 'app', arch: 'aarch64', signature: 'T.app.tar.gz.sig' }] }],
    version: '1.0.0',
    tag: 'v1.0.0',
    repo: REPO,
    signatureOf: () => 'S',
  });
  assert.deepEqual(Object.keys(m.platforms).sort(), ['darwin-aarch64', 'darwin-aarch64-app']);
});

// ------------------------------------------------------------------------------- publish.mjs

/** Enough of GitHub's releases API to test the publish logic against. */
class FakeGitHub {
  constructor() {
    this.releases = [];
    this.nextId = 1;
    this.failUploads = 0; // how many upload calls fail first, leaving a broken file behind
    this.calls = [];
  }

  addRelease({ tag, draft, assets = [] }) {
    const r = { id: this.nextId++, tag_name: tag, draft, html_url: `https://github.invalid/${REPO}/releases/${draft ? 'untagged' : tag}`, assets: [] };
    for (const name of assets) r.assets.push({ id: this.nextId++, name, size: 1, state: 'uploaded' });
    this.releases.push(r);
    return r;
  }

  client({ write }) {
    const reply = (status, data) => ({ ok: status >= 200 && status < 300, status, text: async () => (data === undefined ? '' : JSON.stringify(data)) });
    const fetchImpl = async (url, init) => {
      const u = new URL(url);
      const path = u.pathname.replace(`/repos/${REPO}`, '');
      this.calls.push(`${init.method} ${u.host}${path}`);
      const visible = this.releases.filter((r) => write || !r.draft);
      let m;
      if (init.method === 'GET' && path === '/releases') {
        const page = Number(u.searchParams.get('page') || 1);
        return reply(200, page === 1 ? visible.map(({ assets, ...r }) => r) : []);
      }
      if (init.method === 'GET' && (m = path.match(/^\/releases\/tags\/(.+)$/))) {
        const r = this.releases.find((x) => !x.draft && x.tag_name === decodeURIComponent(m[1]));
        return r ? reply(200, r) : reply(404, { message: 'Not Found' });
      }
      if (!write) return reply(403, { message: 'Resource not accessible by integration' });
      if (init.method === 'POST' && path === '/releases') {
        const body = JSON.parse(init.body);
        assert.equal(body.draft, true, 'the pipeline only ever creates drafts');
        return reply(201, this.addRelease({ tag: body.tag_name, draft: true }));
      }
      if (init.method === 'GET' && (m = path.match(/^\/releases\/(\d+)$/))) {
        return reply(200, this.releases.find((r) => r.id === Number(m[1])));
      }
      if (init.method === 'GET' && (m = path.match(/^\/releases\/(\d+)\/assets$/))) {
        const page = Number(u.searchParams.get('page') || 1);
        return reply(200, page === 1 ? this.releases.find((r) => r.id === Number(m[1])).assets : []);
      }
      if (init.method === 'DELETE' && (m = path.match(/^\/releases\/assets\/(\d+)$/))) {
        for (const r of this.releases) {
          if (!r.draft && r.assets.some((a) => a.id === Number(m[1]))) assert.fail('a file of a published release was deleted');
          r.assets = r.assets.filter((a) => a.id !== Number(m[1]));
        }
        return reply(204);
      }
      if (init.method === 'POST' && (m = path.match(/^\/releases\/(\d+)\/assets$/))) {
        assert.equal(u.host, 'uploads.github.com');
        const r = this.releases.find((x) => x.id === Number(m[1]));
        assert.equal(r.draft, true, 'a file was uploaded to a published release');
        const name = u.searchParams.get('name');
        if (r.assets.some((a) => a.name === name)) return reply(422, { message: 'Validation Failed', errors: [{ code: 'already_exists' }] });
        if (this.failUploads > 0) {
          this.failUploads--;
          r.assets.push({ id: this.nextId++, name, size: 0, state: 'starter' });
          return reply(502, 'Bad Gateway');
        }
        const asset = { id: this.nextId++, name, size: init.body.length, state: 'uploaded' };
        r.assets.push(asset);
        return reply(201, asset);
      }
      return reply(404, { message: `unhandled ${init.method} ${path}` });
    };
    return new GitHub({ token: write ? 'write-token' : 'read-token', repo: REPO, fetchImpl, retryDelayMs: 0 });
  }
}

/** Installers of all platforms plus the assemble job's output, ready for publish. */
function readyToPublish({ updater = false } = {}) {
  const assetsDir = collected({ updater });
  const metaDir = tmp();
  const release = assemble(assembleArgs(assetsDir, { metaDir }));
  return { assetsDir, metaDir, names: release.files.map((f) => f.name).sort() };
}
const publishArgs = (hub, ready) => ({ gh: hub.client({ write: true }), tag: 'v1.2.3', commit: SHA, assetsDir: ready.assetsDir, metaDir: ready.metaDir, log: () => {}, pauseMs: 0 });

test('publish: first run creates one draft holding exactly the built files', async () => {
  const hub = new FakeGitHub();
  const ready = readyToPublish({ updater: true });
  await publish(publishArgs(hub, ready));
  assert.equal(hub.releases.length, 1);
  assert.equal(hub.releases[0].draft, true);
  assert.deepEqual(hub.releases[0].assets.map((a) => a.name).sort(), ready.names);
  assert.ok(ready.names.includes('latest.json'));
});

test('publish: a re-run reuses the draft and replaces its files, leftovers included', async () => {
  const hub = new FakeGitHub();
  hub.addRelease({ tag: 'v1.2.3', draft: true, assets: ['Thoughts_1.2.3_x64-setup.exe', 'left-over-from-an-older-attempt.zip'] });
  hub.addRelease({ tag: 'v1.2.2', draft: false, assets: ['Thoughts_1.2.2_x64-setup.exe'] });
  const ready = readyToPublish();
  await publish(publishArgs(hub, ready));
  assert.equal(hub.releases.length, 2);
  assert.deepEqual(hub.releases[0].assets.map((a) => a.name).sort(), ready.names);
  assert.deepEqual(hub.releases[1].assets.map((a) => a.name), ['Thoughts_1.2.2_x64-setup.exe']); // other releases untouched
});

test('publish: a published release is never touched (re-run, re-run of one job, moved tag)', async () => {
  const hub = new FakeGitHub();
  hub.addRelease({ tag: 'v1.2.3', draft: false, assets: ['Thoughts_1.2.3_x64-setup.exe'] });
  const before = JSON.stringify(hub.releases);
  // Preflight, with a token that can only read:
  await assert.rejects(refusePublished({ gh: hub.client({ write: false }), tag: 'v1.2.3' }), stops(/already published/));
  // The publish job itself, if it were reached anyway:
  await assert.rejects(publish(publishArgs(hub, readyToPublish())), stops(/already published/));
  assert.equal(JSON.stringify(hub.releases), before);
  assert.ok(!hub.calls.some((c) => c.startsWith('POST') || c.startsWith('DELETE')), 'no write was attempted');
});

test('publish: preflight lets a new tag and an existing draft through', async () => {
  const hub = new FakeGitHub();
  await refusePublished({ gh: hub.client({ write: false }), tag: 'v1.2.3' });
  hub.addRelease({ tag: 'v1.2.3', draft: true });
  await refusePublished({ gh: hub.client({ write: false }), tag: 'v1.2.3' });
});

test('publish: two drafts for one tag are left for a person to sort out', async () => {
  const hub = new FakeGitHub();
  hub.addRelease({ tag: 'v1.2.3', draft: true });
  hub.addRelease({ tag: 'v1.2.3', draft: true });
  await assert.rejects(publish(publishArgs(hub, readyToPublish())), stops(/2 draft releases/));
});

test('publish: an upload that fails half-way is cleaned up and retried', async () => {
  const hub = new FakeGitHub();
  hub.failUploads = 2;
  const ready = readyToPublish();
  await publish(publishArgs(hub, ready));
  assert.deepEqual(hub.releases[0].assets.map((a) => `${a.name}:${a.state}`).sort(), ready.names.map((n) => `${n}:uploaded`));
});

test('publish: a file changed after the assemble job checked it is not uploaded', async () => {
  const hub = new FakeGitHub();
  const ready = readyToPublish();
  writeFileSync(join(ready.assetsDir, 'Thoughts_1.2.3_amd64.deb'), 'something else');
  await assert.rejects(publish(publishArgs(hub, ready)), stops(/does not match its checksum/));
  assert.equal(hub.releases.length, 0);

  const other = readyToPublish();
  await assert.rejects(publish({ ...publishArgs(hub, other), tag: 'v1.2.4' }), stops(/are for v1\.2\.3, but this run is for v1\.2\.4/));
  await assert.rejects(publish({ ...publishArgs(hub, other), commit: 'd'.repeat(40) }), stops(/were built from/));
  assert.equal(hub.releases.length, 0);
});

test('publish: a rehearsal file list can never be published', async () => {
  const hub = new FakeGitHub();
  const assetsDir = collected();
  const metaDir = tmp();
  assemble(assembleArgs(assetsDir, { metaDir, mode: 'rehearsal' }));
  await assert.rejects(publish(publishArgs(hub, { assetsDir, metaDir })), stops(/not for a release/));
  assert.equal(hub.releases.length, 0);
});

// ------------------------------------------- notes, the updater key and the release made on a Mac

const CHANGELOG = `# What's new in Thoughts

## 1.2.3

A small release.

### Chats
- **Calmer chats:** the assistant's \`commands\` fold into [one line](https://example.com/x).
- Second thing.

## 1.2.2

Older.
`;

test('notes: the changelog entry of a version becomes the plain-text notes of latest.json (shown by copies before 0.1.0)', () => {
  assert.equal(changelogSection('1.2.2', CHANGELOG), 'Older.');
  assert.equal(changelogSection('9.9.9', CHANGELOG), null);
  assert.equal(changelogSection('1.2', CHANGELOG), null, 'only the exact version');
  assert.equal(notesFor('1.2.3', CHANGELOG), "A small release.\n\nChats\n• Calmer chats: the assistant's commands fold into one line.\n• Second thing.");
  assert.equal(notesFor('9.9.9', CHANGELOG), null);
  const cut = notesFor('1.2.3', CHANGELOG, { max: 40 });
  assert.equal(cut, 'A small release.\n\nChats\n…', 'cut at the end of a line, never in the middle of one');
});

test('assemble: latest.json carries the notes in both languages; without an entry, the plain sentence', () => {
  const metaDir = tmp();
  assemble(assembleArgs(collected({ updater: true }), { metaDir, notes: 'What changed.', notesAr: 'ما الذي تغيّر.' }));
  const latest = JSON.parse(readFileSync(join(metaDir, 'latest.json'), 'utf8'));
  assert.equal(latest.notes, 'What changed.');
  assert.equal(latest.notes_ar, 'ما الذي تغيّر.');
  const plain = tmp();
  assemble(assembleArgs(collected({ updater: true }), { metaDir: plain }));
  const bare = JSON.parse(readFileSync(join(plain, 'latest.json'), 'utf8'));
  assert.match(bare.notes, /See the assets below/);
  assert.ok(!('notes_ar' in bare));
});

test('assemble: a release of one platform is only made when asked for (the release made on a Mac)', () => {
  const mac = collected({ platforms: ['macos'], updater: true });
  assert.throws(() => assemble(assembleArgs(mac, { platforms: ['macos'] })), stops(/needs every platform/));
  const metaDir = tmp();
  const release = assemble(assembleArgs(mac, { metaDir, platforms: ['macos'], partial: true }));
  assert.equal(release.mode, 'release');
  assert.deepEqual(release.files.map((f) => f.name), [
    'Thoughts_1.2.3_universal.app.tar.gz',
    'Thoughts_1.2.3_universal.app.tar.gz.sig',
    'Thoughts_1.2.3_universal.dmg',
    'latest.json',
  ]);
  const latest = JSON.parse(readFileSync(join(metaDir, 'latest.json'), 'utf8'));
  assert.deepEqual(Object.keys(latest.platforms).sort(), ['darwin-aarch64', 'darwin-aarch64-app', 'darwin-universal', 'darwin-universal-app', 'darwin-x86_64', 'darwin-x86_64-app']);
  // The addresses are the public ones, so the same file serves the day the repository opens.
  assert.equal(latest.platforms['darwin-aarch64'].url, `https://github.com/${REPO}/releases/download/v1.2.3/Thoughts_1.2.3_universal.app.tar.gz`);
});

test('release config: the public key comes from the committed file, and two different keys stop the build', () => {
  const on = releaseConfig({ env: { UPDATER_ENDPOINT: 'https://a.example/latest.json, https://b.example/latest.json' }, committedKey: 'KEY\n' });
  assert.deepEqual(on, {
    bundle: { createUpdaterArtifacts: true },
    plugins: { updater: { pubkey: 'KEY', endpoints: ['https://a.example/latest.json', 'https://b.example/latest.json'], requireSignedVersion: true } },
  });
  const env = { UPDATER_PUBKEY: 'KEY', UPDATER_ENDPOINT: 'https://a.example/x' };
  assert.equal(releaseConfig({ env }).plugins.updater.pubkey, 'KEY', 'the variable alone still works');
  assert.equal(releaseConfig({ env, committedKey: 'KEY' }).plugins.updater.pubkey, 'KEY', 'the same key in both places is fine');
  assert.throws(() => releaseConfig({ env: { ...env, UPDATER_PUBKEY: 'OTHER' }, committedKey: 'KEY' }), /different keys/);
  // No key, or no address: a build like a local one, with the updater off.
  assert.deepEqual(releaseConfig({ env: { UPDATER_ENDPOINT: 'https://a.example/x' } }), {});
  assert.deepEqual(releaseConfig({ env: {}, committedKey: 'KEY' }), {});
  // The second call adds Windows signing to what the first wrote.
  const win = releaseConfig({ env: { WINDOWS_CERTIFICATE_THUMBPRINT: 'ABC' }, existing: structuredClone(on), addWindowsSigning: true });
  assert.equal(win.plugins.updater.pubkey, 'KEY');
  assert.equal(win.bundle.createUpdaterArtifacts, true);
  assert.equal(win.bundle.windows.certificateThumbprint, 'ABC');
  assert.throws(() => releaseConfig({ env: {}, addWindowsSigning: true }), /does not exist yet/);
});

// A throwaway key made for these tests, and the signature it gave a file holding "hello update"
// for version 1.2.3 (`tauri signer sign --app-version 1.2.3`).
const TEST_PUB = 'dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDMwQ0E3RjQwNkZFNjBFNTAKUldSUUR1WnZRSC9LTUF1WnhiMkNhRjhqYmgrN0tDSUtxOWN1VU9wc0J1bDdJREE1azM3a29zVmIK';
const TEST_SIG = 'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSUUR1WnZRSC9LTU12UHJhenV2UzUrbWExN0pzWTdjOS9jZWd1UjVobkJJc1NWN0hmVURyNE9hZm9QVEFhQWZTL05IL3BXV2xla2owUW5tQ05SM25TRUM4UzJCUjZvVFFVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0NzY3ODA3CWZpbGU6YXBwLnRhci5negl2ZXJzaW9uOjEuMi4zCmtqanNrVFNGRGQ2b21FWVQ0WTk0VW0xU2pDK3llT1k0YjQyYUN3WFR6ZHl4UnVVTVJ4NXA5R1dPcW5kdC82enpUZUVnakpJMlJURVBWcmt2ZWRtYkN3PT0K';

test('signatures: an update file is accepted only as signed, by this key, and says its version', () => {
  const data = Buffer.from('hello update');
  assert.deepEqual(checkSignature({ publicKeyFile: TEST_PUB, signatureFile: TEST_SIG, data }), { ok: true, version: '1.2.3', file: 'app.tar.gz' });
  assert.match(checkSignature({ publicKeyFile: TEST_PUB, signatureFile: TEST_SIG, data: Buffer.from('hello updatE') }).why, /does not match the file/);
  // The version sits in a comment that is itself signed: rewriting it is noticed.
  const lines = Buffer.from(TEST_SIG, 'base64').toString('utf8').split('\n');
  lines[2] = lines[2].replace('version:1.2.3', 'version:9.9.9');
  const rewritten = Buffer.from(lines.join('\n')).toString('base64');
  assert.match(checkSignature({ publicKeyFile: TEST_PUB, signatureFile: rewritten, data }).why, /comment was changed/);
  // Another key: the same bytes with the key's last byte changed.
  const key = Buffer.from(TEST_PUB, 'base64').toString('utf8').split('\n');
  const raw = Buffer.from(key[1], 'base64');
  raw[raw.length - 1] ^= 1;
  key[1] = raw.toString('base64');
  assert.equal(checkSignature({ publicKeyFile: Buffer.from(key.join('\n')).toString('base64'), signatureFile: TEST_SIG, data }).ok, false);
  assert.equal(checkSignature({ publicKeyFile: 'garbage', signatureFile: TEST_SIG, data }).ok, false);
  assert.equal(checkSignature({ publicKeyFile: TEST_PUB, signatureFile: 'garbage', data }).ok, false);
});

test('local release: what must hold before anything is built', () => {
  const ready = { branch: 'main', dirty: false, version: '1.2.3', head: SHA, tagAt: null, key: 'KEY', password: 'pw', publicKey: 'PUB', keysMatch: true, changelog: 'entry', changelogAr: 'مدخل' };
  assert.deepEqual(preflightProblems(ready), []);
  assert.deepEqual(preflightProblems({ ...ready, tagAt: SHA, password: '' }), [], 'a tag on this very commit and an empty password are fine');
  const one = (change, pattern) => {
    const found = preflightProblems({ ...ready, ...change });
    assert.equal(found.length, 1, JSON.stringify(found));
    assert.match(found[0], pattern);
  };
  one({ branch: 'r12-updates' }, /made from main/);
  one({ dirty: true }, /not committed/);
  one({ version: '1.2' }, /not a version/);
  one({ tagAt: 'b'.repeat(40) }, /already exists on another commit/);
  one({ key: null }, /private key was not found/);
  one({ password: null }, /password was not found/);
  one({ publicKey: '' }, /updater\.pub is missing/);
  one({ keysMatch: false }, /does not match/);
  one({ changelog: null }, /docs\/CHANGELOG\.md has no entry "## 1\.2\.3"/);
  one({ changelogAr: null }, /docs\/ar\/CHANGELOG\.md has no entry/);
});

test('local release: every test suite runs before the build, the UI tests included', () => {
  const lines = TESTS.map(([cmd, args]) => [cmd === process.execPath ? 'node' : cmd, ...args].join(' '));
  assert.deepEqual(lines, [
    'cargo test -p thoughtsd',
    'cargo test -p thoughts',
    'npm run check',
    'npx vitest run',
    `node --test ${join(ROOT, 'scripts', 'release', 'release.test.mjs')}`,
  ]);
  // The build itself runs these from the list, not a copy of it.
  const source = readFileSync(join(ROOT, 'scripts', 'release', 'local.mjs'), 'utf8');
  const main = source.slice(source.indexOf('if (isMain(import.meta.url))'));
  assert.match(main, /for \(const \[cmd, cmdArgs\] of TESTS\) sh\(cmd, cmdArgs\)/);
  assert.ok(main.indexOf('of TESTS') < main.indexOf('built = build(f)'), 'the tests come before the build');
});

test('local release: the repository, the key and the workflows are found where they are', () => {
  assert.equal(repoFromRemote('https://github.com/someone/thoughts.git\n'), 'someone/thoughts');
  assert.equal(repoFromRemote('git@github.com:someone/thoughts.git'), 'someone/thoughts');
  assert.equal(repoFromRemote('https://github.com/someone/thoughts'), 'someone/thoughts');
  assert.equal(repoFromRemote('https://example.com/someone/thoughts.git'), null);
  assert.equal(publicEndpoint('someone/thoughts'), 'https://github.com/someone/thoughts/releases/latest/download/latest.json');

  const files = { '/keys/updater.key': 'FROM-FILE\n', '/usual/place.key': 'USUAL\n' };
  const fs = { exists: (p) => p in files, read: (p) => files[p], fallback: '/usual/place.key' };
  assert.equal(findKey({ TAURI_SIGNING_PRIVATE_KEY: '/keys/updater.key' }, fs), 'FROM-FILE', 'a path is read');
  assert.equal(findKey({ TAURI_SIGNING_PRIVATE_KEY: 'THE-KEY-ITSELF' }, fs), 'THE-KEY-ITSELF');
  assert.equal(findKey({}, fs), 'USUAL');
  assert.equal(findKey({}, { ...fs, fallback: '/nowhere' }), null);

  const listed = [
    { name: 'CI', state: 'active', id: 1 },
    { name: 'Release', state: 'disabled_manually', id: 2 },
    { name: 'Advisories', state: 'active', id: 3 },
    { name: 'Dependabot Updates', state: 'active', id: 4 },
  ];
  assert.deepEqual(workflowsToSwitch(listed), [{ id: '1', name: 'CI' }, { id: '3', name: 'Advisories' }], 'only those a push starts, and only those that are on');
  assert.deepEqual(workflowsToSwitch(null), []);
});

test('local release: commits someone else made are listed, and the build waits until they are read', () => {
  const line = (...fields) => fields.join('\x1f');
  const log = [
    line('aaa1111', 'Me', 'me@example.com', 'me@example.com', 'my own change'),
    line('bbb2222', 'Friend', 'friend@example.com', 'friend@example.com', "a friend's change"),
    line('ccc3333', 'Me', 'me@example.com', 'noreply@github.com', 'made on the website'),
    line('ddd4444', 'Friend', 'friend@example.com', 'ME@example.com', "a friend's change that I applied"),
  ].join('\n');
  const others = commitsByOthers(log, ['Me@Example.com']);
  assert.deepEqual(others.map((c) => c.id), ['bbb2222', 'ccc3333', 'ddd4444'], 'the author and the committer must both be the person releasing');
  assert.deepEqual(commitsByOthers(log, ['me@example.com', 'noreply@github.com', 'friend@example.com']), []);
  assert.deepEqual(commitsByOthers('', ['me@example.com']), []);

  const ready = { branch: 'main', dirty: false, version: '1.2.3', head: SHA, tagAt: null, key: 'KEY', password: 'pw', publicKey: 'PUB', keysMatch: true, changelog: 'entry', changelogAr: 'مدخل' };
  assert.deepEqual(preflightProblems({ ...ready, others: [], behind: 0 }), []);
  const waiting = preflightProblems({ ...ready, others, since: 'v1.2.2' });
  assert.equal(waiting.length, 1);
  assert.match(waiting[0], /^3 commits since v1\.2\.2 were not made by you:/);
  assert.match(waiting[0], /bbb2222 {2}Friend <friend@example\.com> {2}a friend's change/);
  assert.match(waiting[0], /--include-others/);
  assert.deepEqual(preflightProblems({ ...ready, others, since: 'v1.2.2', includeOthers: true }), [], 'said to be read: the build goes on');
  assert.match(preflightProblems({ ...ready, others: others.slice(0, 1), since: 'v1.2.2' })[0], /^1 commit since v1\.2\.2 was not made by you:/);

  // Someone pushed to GitHub's main and this computer has not seen it yet.
  const behind = preflightProblems({ ...ready, behind: 2 });
  assert.equal(behind.length, 1);
  assert.match(behind[0], /GitHub's main has 2 commits this Mac does not have/);
});

// A stand-in for /usr/bin/security: a set of keychains, each locked or not, holding items
// { service, account, password }. Records every call, so a test can see what was asked and when.
// Never touches a real keychain.
function fakeSecurity({ keychains = {}, deny = false, unlockPassword = 'right', typed = 'right' } = {}) {
  const LOGIN = '/Users/me/Library/Keychains/login.keychain-db';
  const chains = { [LOGIN]: { locked: false, items: [] }, ...keychains };
  const calls = [];
  const run = (args, opts = {}) => {
    calls.push({ args, interactive: !!opts.interactive, locked: Object.fromEntries(Object.entries(chains).map(([k, v]) => [k, v.locked])) });
    const [cmd, ...rest] = args;
    if (cmd === 'login-keychain') return { ok: true, out: `"${LOGIN}"` };
    if (cmd === 'unlock-keychain' || cmd === 'lock-keychain') {
      const chain = chains[rest.at(-1)];
      if (!chain) return { ok: false, out: '' };
      if (cmd === 'unlock-keychain') {
        if (!opts.interactive || typed !== unlockPassword) return { ok: false, out: '' };
        chain.locked = false;
      } else chain.locked = true;
      return { ok: true, out: '' };
    }
    if (cmd === 'find-generic-password') {
      const get = (flag) => (rest.includes(flag) ? rest[rest.indexOf(flag) + 1] : undefined);
      const reveal = rest.includes('-w');
      const last = rest.at(-1);
      const where = last in chains ? [last] : [LOGIN, ...Object.keys(keychains).filter((k) => keychains[k].inSearchList)];
      for (const name of where) {
        const chain = chains[name];
        const item = chain.items.find((i) => i.service === get('-s') && (get('-a') === undefined || i.account === get('-a')));
        if (!item) continue;
        if (reveal && (chain.locked || (item.alwaysAsk && deny))) return { ok: false, out: '' };
        return { ok: true, out: reveal ? item.password : `keychain: "${name}"` };
      }
      return { ok: false, out: '' };
    }
    throw new Error(`unexpected security ${args.join(' ')}`);
  };
  return { run, calls, chains, LOGIN };
}
const item = (account, password, extra = {}) => ({ service: KEYCHAIN_SERVICE, account, password, ...extra });

test('key password: the environment comes first, with a warning not to keep it in a file', () => {
  const sec = fakeSecurity();
  const r = findPassword({ TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'from-env' }, { platform: 'darwin', run: sec.run });
  assert.equal(r.password, 'from-env');
  assert.equal(r.source, 'environment');
  assert.match(r.warnings.join(' '), /shell profile/);
  assert.equal(sec.calls.length, 0, 'no keychain is asked');
  assert.equal(findPassword({ TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' }, { platform: 'darwin', run: sec.run }).password, '', 'an empty password is a password');
  assert.equal(findPassword({}, { platform: 'linux', run: sec.run }).password, null, 'no keychain off macOS');
});

test('key password: the old login-keychain item still works, with a warning to move it', () => {
  const sec = fakeSecurity();
  sec.chains[sec.LOGIN].items.push(item('release', 'old-way'));
  const r = findPassword({}, { platform: 'darwin', run: sec.run });
  assert.equal(r.password, 'old-way');
  assert.equal(r.source, 'login-keychain');
  assert.equal(r.problem, null);
  assert.equal(r.warnings.length, 1);
  assert.match(r.warnings[0], /without being asked/);
  assert.match(r.warnings[0], /always-ask item or to a separate keychain/);
  assert.match(r.warnings[0], /off the account you use every day/);
  assert.match(r.warnings[0], /Keeping the updater key safe/);
});

test('key password: an always-ask item is read once macOS asked; a Deny never falls back to the old item', () => {
  const sec = fakeSecurity();
  sec.chains[sec.LOGIN].items.push(item(ALWAYS_ASK_ACCOUNT, 'asked', { alwaysAsk: true }));
  const r = findPassword({}, { platform: 'darwin', run: sec.run });
  assert.equal(r.password, 'asked');
  assert.equal(r.source, 'always-ask');
  assert.deepEqual(r.warnings, [], 'moved: nothing to warn about');
  // Whether it exists is looked up without -w (no question, no password), then it is read.
  const reads = sec.calls.filter((c) => c.args[0] === 'find-generic-password');
  assert.ok(!reads[0].args.includes('-w'));
  assert.ok(reads[1].args.includes('-w') && reads[1].args.includes(ALWAYS_ASK_ACCOUNT));

  // The old item is still there: works, and says to delete it.
  const both = fakeSecurity();
  both.chains[both.LOGIN].items.push(item(ALWAYS_ASK_ACCOUNT, 'asked', { alwaysAsk: true }), item('release', 'old-way'));
  const r2 = findPassword({}, { platform: 'darwin', run: both.run });
  assert.equal(r2.password, 'asked');
  assert.match(r2.warnings.join(' '), /old keychain item .* is still in your login keychain/);
  assert.ok(!both.calls.some((c) => c.args.includes('release') && c.args.includes('-w')), 'the old item is looked up, never read');

  // Deny: no password, a clear reason, and the old item is not read instead.
  const denied = fakeSecurity({ deny: true });
  denied.chains[denied.LOGIN].items.push(item(ALWAYS_ASK_ACCOUNT, 'asked', { alwaysAsk: true }), item('release', 'old-way'));
  const r3 = findPassword({}, { platform: 'darwin', run: denied.run });
  assert.equal(r3.password, null);
  assert.match(r3.problem, /choose Allow \(never Always Allow/);
  assert.match(preflightProblems({ branch: 'main', version: '1.2.3', key: 'KEY', password: r3.password, passwordProblem: r3.problem, publicKey: 'PUB', changelog: 'x', changelogAr: 'y' })[0], /choose Allow/);
});

test('key password: a separate keychain is unlocked by asking, read, and locked again before anything else', () => {
  const RELEASE = '/Users/me/Library/Keychains/thoughts-release.keychain-db';
  const sec = fakeSecurity({ keychains: { [RELEASE]: { locked: true, items: [item('release', 'separate')] } } });
  const r = findPassword({ THOUGHTS_UPDATER_KEYCHAIN: RELEASE }, { platform: 'darwin', run: sec.run });
  assert.equal(r.password, 'separate');
  assert.equal(r.source, 'separate-keychain');
  assert.deepEqual(r.warnings, []);
  assert.equal(sec.chains[RELEASE].locked, true, 'locked again');
  const order = sec.calls.map((c) => c.args[0]);
  assert.deepEqual(order.slice(0, 3), ['unlock-keychain', 'find-generic-password', 'lock-keychain']);
  assert.equal(sec.calls[0].interactive, true, 'unlocking asks in the terminal');
  assert.equal(sec.calls[1].args.at(-1), RELEASE, 'read from that keychain only');

  // Wrong password typed: nothing is read, and the reason names the keychain.
  const wrong = fakeSecurity({ typed: 'wrong', keychains: { [RELEASE]: { locked: true, items: [item('release', 'separate')] } } });
  const r2 = findPassword({ THOUGHTS_UPDATER_KEYCHAIN: RELEASE }, { platform: 'darwin', run: wrong.run });
  assert.equal(r2.password, null);
  assert.match(r2.problem, /Could not unlock the keychain .*thoughts-release/);
  assert.ok(!wrong.calls.some((c) => c.args.includes('-w')));

  // The keychain has no such item: still locked again afterwards.
  const empty = fakeSecurity({ keychains: { [RELEASE]: { locked: true, items: [] } } });
  const r3 = findPassword({ THOUGHTS_UPDATER_KEYCHAIN: RELEASE }, { platform: 'darwin', run: empty.run });
  assert.equal(r3.password, null);
  assert.match(r3.problem, /holds no item "thoughts-updater"/);
  assert.equal(empty.chains[RELEASE].locked, true);

  // Moved, but the old login item was left behind.
  const left = fakeSecurity({ keychains: { [RELEASE]: { locked: true, items: [item('release', 'separate')] } } });
  left.chains[left.LOGIN].items.push(item('release', 'old-way'));
  assert.match(findPassword({ THOUGHTS_UPDATER_KEYCHAIN: RELEASE }, { platform: 'darwin', run: left.run }).warnings.join(' '), /old keychain item/);

  assert.equal(keychainPath('    "/Users/me/Library/Keychains/login.keychain-db"\n'), '/Users/me/Library/Keychains/login.keychain-db');
});

test('key password: nothing anywhere is "not found", and the problem points to the docs, not a command', () => {
  const sec = fakeSecurity();
  const r = findPassword({}, { platform: 'darwin', run: sec.run });
  assert.equal(r.password, null);
  const [problem] = preflightProblems({ branch: 'main', version: '1.2.3', key: 'KEY', password: null, publicKey: 'PUB', changelog: 'x', changelogAr: 'y' });
  assert.match(problem, /password was not found/);
  assert.match(problem, /Keeping the updater key safe/);
  assert.doesNotMatch(problem, /add-generic-password/);
});

// ------------------------------------------------- compile and sign apart (release.yml, tauri-ci)
//
// The workflow cannot be run here, so its shape is checked as text: which job gets which secret,
// what the sign job runs, and that every action is pinned. A plain splitter, not a YAML parser
// (these scripts have no dependencies): jobs are the two-space keys under `jobs:`, steps the
// `      - ` items, comment lines are ignored.

const WORKFLOW = readFileSync(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8');
function jobsOf(text) {
  const lines = text.split('\n').filter((l) => !/^\s*#/.test(l));
  const start = lines.indexOf('jobs:');
  const jobs = {};
  let name = null;
  for (const line of lines.slice(start + 1)) {
    const m = line.match(/^ {2}([a-z][a-z-]*):\s*$/);
    if (m) jobs[(name = m[1])] = [];
    else if (name) jobs[name].push(line.replace(/\s+#.*$/, ''));
  }
  return Object.fromEntries(Object.entries(jobs).map(([k, v]) => [k, v.join('\n')]));
}
const stepsOf = (job) => job.split(/\n(?= {6}- )/).slice(1);
const stepName = (step) => (step.match(/name: (.+)/) || step.match(/uses: (.+)/) || [, '?'])[1].trim();

test('release.yml: the compile job never sees a secret; the sign job sees them in two steps only', () => {
  const jobs = jobsOf(WORKFLOW);
  assert.ok(jobs.compile && jobs.sign, 'a compile job and a sign job');
  assert.ok(!('build' in jobs), 'no job compiles and signs at once any more');
  assert.doesNotMatch(jobs.compile, /secrets\./, 'not one secret in the compile job');
  assert.match(jobs.sign, /needs: \[preflight, compile\]/);
  assert.match(jobs.assemble, /needs: \[preflight, sign\]/);

  const withSecrets = stepsOf(jobs.sign).filter((s) => /secrets\./.test(s)).map(stepName);
  assert.deepEqual(withSecrets, ['Windows signing certificate', 'Bundle and sign']);
  // Elsewhere a secret is only ever compared with '' (its value never enters a step).
  for (const [name, job] of Object.entries(jobs)) {
    if (name === 'sign') continue;
    for (const use of job.match(/secrets\.[A-Z_]+[^}&|]*/g) || []) assert.match(use, /secrets\.[A-Z_]+ != ''/, `${name}: ${use}`);
  }
  for (const job of Object.values(jobs)) assert.doesNotMatch(job, /secrets: inherit/);
});

test('release.yml: the sign job starts fresh and runs no dependency code but the Tauri CLI', () => {
  const sign = jobsOf(WORKFLOW).sign;
  const steps = stepsOf(sign);
  assert.match(stepName(steps[0]), /^actions\/checkout@/, 'a fresh checkout first');
  assert.match(steps[0], /persist-credentials: false/);
  assert.doesNotMatch(sign, /rust-cache|cache:/, 'no build cache and no npm cache');
  const runs = steps.map((s) => (s.match(/run: (?:>-|\|)?\s*([\s\S]*)/) || [, ''])[1].trim()).filter(Boolean);
  const npm = runs.filter((r) => /\bnpm\b|\bnpx\b|\byarn\b|\bpnpm\b/.test(r));
  assert.deepEqual(npm, ['npm ci --ignore-scripts --no-audit --no-fund'], 'one npm command, with install scripts off');
  assert.ok(!runs.some((r) => /cargo (build|run|test)|tauri-ci\.mjs build|vite/.test(r)), 'nothing is compiled or built');
  assert.ok(runs.some((r) => /^node scripts\/tauri-ci\.mjs bundle /.test(r)));
  assert.ok(runs.some((r) => /^node scripts\/release\/compiled\.mjs place /.test(r)));
  // The secrets come after the executable is in place and checked.
  const names = steps.map(stepName);
  assert.ok(names.indexOf('Put the compiled app in place') < names.indexOf('Windows signing certificate'));
  assert.ok(names.indexOf('Windows signing certificate') < names.indexOf('Bundle and sign'));
});

test('release.yml: every action is pinned to a full commit', () => {
  const uses = [...WORKFLOW.matchAll(/^\s*(?:- )?uses: (\S+)/gm)].map((m) => m[1]);
  assert.ok(uses.length > 10);
  for (const u of uses) assert.match(u, /^(\.\/\.github\/workflows\/[\w.-]+\.yml|[\w.-]+\/[\w.-]+@[0-9a-f]{40})$/, u);
});

test('tauri-ci: the compile step loses the secrets, the tokens, the runner file commands and NODE_OPTIONS', () => {
  const given = {
    PATH: '/bin', HOME: '/home/me', CARGO_HOME: '/c',
    TAURI_SIGNING_PRIVATE_KEY: 'KEY', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'pw', APPLE_CERTIFICATE: 'cert', WINDOWS_CERTIFICATE: 'pfx',
    GITHUB_TOKEN: 't', GH_TOKEN: 't', NODE_OPTIONS: '--import=/tmp/x.mjs',
    ...Object.fromEntries(RUNNER_CHANNELS.map((k) => [k, `/runner/${k}`])),
  };
  const r = prepare(['build', '--no-bundle', '--config', 'src-tauri/release.conf.json'], given, { platform: 'linux' });
  assert.deepEqual(Object.keys(r.env).sort(), ['CARGO_HOME', 'HOME', 'PATH']);
  assert.deepEqual(r.args, ['build', '--no-bundle', '--config', 'src-tauri/release.conf.json'], 'arguments unchanged');
  assert.match(r.log.join('\n'), /::warning::The compile step was given APPLE_CERTIFICATE, TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD, WINDOWS_CERTIFICATE\./);
  assert.match(r.log.join('\n'), /removed GITHUB_ENV, GITHUB_PATH, GITHUB_OUTPUT, GITHUB_STATE/);
  assert.match(r.log.join('\n'), /NODE_OPTIONS/);
  assert.doesNotMatch(r.log.join('\n'), /KEY'|pw|\/tmp\/x/, 'names only, never values');
  assert.equal(given.NODE_OPTIONS, '--import=/tmp/x.mjs', 'the caller\'s environment is not changed');
});

test('tauri-ci: the bundle step keeps usable secrets, drops the rest, and runs no hook command', () => {
  const r = prepare(['bundle', '--config', 'src-tauri/release.conf.json'], {
    PATH: '/bin', TAURI_SIGNING_PRIVATE_KEY: 'KEY', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '',
    APPLE_CERTIFICATE: '', APPLE_SIGNING_IDENTITY: '', APPLE_ID: 'me', NODE_OPTIONS: '--require x', GITHUB_ENV: '/f', GITHUB_TOKEN: 't',
  }, { platform: 'darwin' });
  assert.equal(r.env.TAURI_SIGNING_PRIVATE_KEY, 'KEY');
  assert.equal(r.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD, '', 'an empty password is kept with its key');
  for (const k of ['APPLE_CERTIFICATE', 'APPLE_SIGNING_IDENTITY', 'APPLE_ID', 'NODE_OPTIONS', 'GITHUB_ENV', 'GITHUB_TOKEN']) assert.ok(!(k in r.env), k);
  assert.equal(r.env.TAURI_BUNDLER_DMG_IGNORE_CI, 'true');
  assert.deepEqual(r.args.slice(-2), ['--config', NO_HOOKS]);
  assert.deepEqual(JSON.parse(NO_HOOKS), { build: { beforeBundleCommand: null, beforeBuildCommand: null } });
  const none = prepare(['bundle'], { TAURI_SIGNING_PRIVATE_KEY: ' ', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'pw' }, { platform: 'linux' });
  assert.ok(!('TAURI_SIGNING_PRIVATE_KEY' in none.env) && !('TAURI_SIGNING_PRIVATE_KEY_PASSWORD' in none.env), 'no key: no password either');
});

test('compiled app hand-over: only the checked executable reaches the sign job', () => {
  for (const id of ['macos', 'windows', 'linux']) {
    const compiler = tmp();
    const exe = executable(id, compiler);
    mkdirSync(join(exe.path, '..'), { recursive: true });
    writeFileSync(exe.path, `binary for ${id}`);
    mkdirSync(join(compiler, 'target', 'release', 'build', 'evil'), { recursive: true }); // never handed over
    const out = join(tmp(), 'compiled');
    const record = pack({ platform: id, root: compiler, out });
    assert.equal(record.file, id === 'windows' ? 'thoughts.exe' : 'thoughts');
    if (id === 'macos') assert.match(exe.path, /target\/universal-apple-darwin\/release\/thoughts$/);

    const fresh = tmp();
    const placed = place({ platform: id, root: fresh, from: out });
    assert.equal(readFileSync(placed.path, 'utf8'), `binary for ${id}`);
    if (process.platform !== 'win32') assert.equal(statSync(placed.path).mode & 0o777, 0o755, 'executable again');
    assert.throws(() => place({ platform: id, root: fresh, from: out }), stops(/already exists/), 'a fresh checkout only');
  }

  const make = () => {
    const compiler = tmp();
    const exe = executable('linux', compiler);
    mkdirSync(join(exe.path, '..'), { recursive: true });
    writeFileSync(exe.path, 'binary');
    const out = join(tmp(), 'compiled');
    pack({ platform: 'linux', root: compiler, out });
    return out;
  };
  let out = make();
  writeFileSync(join(out, 'tauri.js'), 'planted');
  assert.throws(() => place({ platform: 'linux', root: tmp(), from: out }), stops(/exactly compiled\.json and thoughts; found compiled\.json, tauri\.js, thoughts/));
  out = make();
  writeFileSync(join(out, 'thoughts'), 'changed on the way');
  assert.throws(() => place({ platform: 'linux', root: tmp(), from: out }), stops(/not the file the compile job recorded/));
  assert.throws(() => place({ platform: 'macos', root: tmp(), from: make() }), stops(/describes thoughts for linux, not thoughts for macos/), 'another platform\'s file');
  if (process.platform !== 'win32') {
    out = join(tmp(), 'compiled');
    mkdirSync(out);
    symlinkSync('/etc/hosts', join(out, 'thoughts'));
    writeFileSync(join(out, MANIFEST), '{}');
    assert.throws(() => place({ platform: 'linux', root: tmp(), from: out }), stops(/not a plain file/));
  }
  assert.throws(() => place({ platform: 'linux', root: tmp(), from: join(tmp(), 'nothing') }), stops(/was not handed over/));
  const empty = tmp();
  assert.throws(() => pack({ platform: 'linux', root: empty, out: join(tmp(), 'o') }), stops(/compiled app is missing/));
  assert.ok(!existsSync(join(empty, 'target')));
});
