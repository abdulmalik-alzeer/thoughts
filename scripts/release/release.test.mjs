// Tests for the release pipeline's own scripts. The workflows cannot be run on a developer's
// machine, so everything they decide is in plain functions that can:
//
//   node --test scripts/release/release.test.mjs        (CI runs this in the "UI check" job)
//
// GitHub is replaced by FakeGitHub, a tiny in-memory stand-in that answers the handful of REST
// calls the scripts make, with the rules that matter here (a read-only token cannot see drafts,
// a draft cannot be fetched by tag, a file name can only be uploaded once).
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { assemble, updaterManifest } from './assemble.mjs';
import { decide, verdict } from './ci-status.mjs';
import { collect } from './collect.mjs';
import { GitHub, PLATFORMS, Stop } from './lib.mjs';
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
