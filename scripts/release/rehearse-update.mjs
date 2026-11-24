#!/usr/bin/env node
// Rehearses an in-app update on this Mac, start to finish, without touching the installed app,
// its folder, the real key or GitHub. Run it before any release that changes update code:
//
//   node scripts/release/rehearse-update.mjs            every scenario, then clean up
//   node scripts/release/rehearse-update.mjs --keep     leave the work folder and its report
//   node scripts/release/rehearse-update.mjs --only public,tampered
//   node scripts/release/rehearse-update.mjs --where ~/Applications
//   node scripts/release/rehearse-update.mjs --identity "Name of a signing certificate"
//   node scripts/release/rehearse-update.mjs --by-hand
//       the old version opens and waits for you to choose Check for Updates… yourself
//
// What it does:
//   1. copies the working tree (as it is, uncommitted changes included) to a temporary folder;
//   2. makes a throwaway signing key and builds the app twice there, as 0.0.90 and 0.0.91, under
//      its own name ("Thoughts Rehearsal") and identifier, with the `rehearsal` feature: such a
//      build looks for an update and installs it by itself and writes down each step;
//   3. serves a pretend "public address" (the public release's latest.json) from this computer;
//   4. for each scenario installs 0.0.90 into the Applications folder, starts it, and reads what
//      it wrote: did it become 0.0.91, how, and what did macOS and the app refuse.
//
// A window named "Thoughts Rehearsal" opens and closes a few times while it runs. If macOS asks
// anything (a password, permission to change apps, the keychain), that is a finding: note it and
// answer as you would. The script only sees that a step did not finish by itself.
//
// Not covered here, check by hand: the microphone permission after an update, an update while a
// chat is running, a copy downloaded through a browser (the quarantine mark), other Macs.
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { isMain, ROOT, run, stop } from './lib.mjs';
import { checkSignature } from './minisign.mjs';

const NAME = 'Thoughts Rehearsal';
const IDENTIFIER = 'app.thoughts.rehearsal';
const [OLD, NEW] = ['0.0.90', '0.0.91'];
const ARCH = process.arch === 'arm64' ? 'aarch64' : 'x86_64';
const ARCHIVE = `Thoughts-Rehearsal_${NEW}_${ARCH}.app.tar.gz`;
const KEYCHAIN = 'thoughts-rehearsal';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function sh(cmd, args, { cwd = ROOT, env = process.env, quiet = true, allowFail = false } = {}) {
  const r = spawnSync(cmd, args, { cwd, env, encoding: 'utf8', stdio: quiet ? ['ignore', 'pipe', 'pipe'] : 'inherit', maxBuffer: 64 * 1024 * 1024 });
  if (r.error) stop(`Could not run ${cmd}: ${r.error.message}`);
  if (r.status !== 0 && !allowFail) stop(`${cmd} ${args.slice(0, 4).join(' ')} failed:\n${((r.stderr || '') + (r.stdout || '')).trim().split('\n').slice(-25).join('\n')}`);
  return { ok: r.status === 0, out: (r.stdout || '').trim(), err: (r.stderr || '').trim() };
}

// ------------------------------------------------------------------- the pretend public address

function startFeed(state) {
  const server = createServer((req, res) => {
    const path = new URL(req.url, 'http://127.0.0.1').pathname;
    state.seen.push({ path, signedIn: !!req.headers.authorization });
    const send = (code, body = '', headers = {}) => {
      res.writeHead(code, { 'Content-Length': Buffer.byteLength(body), Connection: 'close', ...headers });
      res.end(body);
    };
    const json = (code, v) => send(code, JSON.stringify(v), { 'Content-Type': 'application/json' });
    const file = (name) => {
      const asset = state.assets.find((a) => a.name === decodeURIComponent(name));
      if (!asset) return json(404, { message: 'Not Found' });
      let bytes = asset.bytes;
      if (state.tamper && asset.name.endsWith('.tar.gz')) {
        bytes = Buffer.from(bytes);
        bytes[Math.floor(bytes.length / 2)] ^= 1; // one changed byte
      }
      res.writeHead(200, { 'Content-Length': bytes.length, 'Content-Type': 'application/octet-stream', Connection: 'close' });
      res.end(bytes);
    };

    // The public address: what a copy asks first, with no sign-in.
    if (path === '/public/latest.json') {
      if (state.public === 'silent') return req.socket.destroy(); // as if there were no connection
      if (state.public === 'notfound') return json(404, { message: 'Not Found' }); // nothing published
      return json(200, state.manifest('public'));
    }
    if (path.startsWith('/public/files/')) return file(path.slice('/public/files/'.length));
    return json(404, { message: 'Not Found' });
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// --------------------------------------------------------------------------------- the builds

/** Copy the working tree (what git tracks, plus new files it does not ignore) to `to`. */
function copyTree(to) {
  const listed = spawnSync('git', ['ls-files', '-z', '-co', '--exclude-standard'], { cwd: ROOT, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (listed.status !== 0) stop('git ls-files failed');
  for (const rel of listed.stdout.split('\0').filter(Boolean)) {
    const from = join(ROOT, rel);
    if (!existsSync(from)) continue; // deleted but not committed yet
    mkdirSync(dirname(join(to, rel)), { recursive: true });
    cpSync(from, join(to, rel));
  }
  symlinkSync(join(ROOT, 'node_modules'), join(to, 'node_modules'));
}

function buildOne(src, version, key) {
  sh(process.execPath, ['scripts/set-version.mjs', version], { cwd: src });
  const cli = join(ROOT, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
  // The repository's own target folder: everything the app depends on is compiled there already.
  const env = { ...process.env, CARGO_TARGET_DIR: join(ROOT, 'target'), TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' };
  console.log(`Building ${NAME} ${version} …`);
  sh(process.execPath, [cli, 'build', '--debug', '--ci', '--bundles', 'app', '--features', 'rehearsal', '--config', 'src-tauri/rehearsal.conf.json'], { cwd: src, env });
  return join(ROOT, 'target', 'debug', 'bundle', 'macos');
}

// ------------------------------------------------------------------------------ one scenario

const lines = (file) => (existsSync(file) ? readFileSync(file, 'utf8').split('\n').filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean) : []);

async function waitFor(file, done, limitMs) {
  const until = Date.now() + limitMs;
  while (Date.now() < until) {
    const seen = lines(file);
    if (done(seen)) return { seen, finished: true };
    await sleep(500);
  }
  return { seen: lines(file), finished: false };
}

const bundleVersion = (app) => sh('plutil', ['-extract', 'CFBundleShortVersionString', 'raw', '-o', '-', join(app, 'Contents', 'Info.plist')], { allowFail: true }).out;

function stopCopy(app) {
  // Only processes started from this rehearsal copy: the pattern is its full path.
  sh('pkill', ['-f', join(app, 'Contents', 'MacOS', 'thoughts')], { allowFail: true });
}

async function scenario(ctx, { id, title, source = 'public', tamper = false, save = false, readOnly = false, expectVersion, limitMs = 150000 }) {
  const { work, state, builds } = ctx;
  const home = join(work, 'homes', id);
  const log = join(work, 'logs', `${id}.jsonl`);
  mkdirSync(home, { recursive: true });
  Object.assign(state, { public: source, tamper, seen: [] });

  // Install 0.0.90 afresh.
  let app = join(ctx.where, `${NAME}.app`);
  stopCopy(app);
  rmSync(app, { recursive: true, force: true });
  sh('ditto', [join(builds, OLD, `${NAME}.app`), app]);
  let mount = null;
  if (readOnly) {
    // Opened straight from a disk image: a place the copy cannot write to.
    const dmg = join(work, `${id}.dmg`);
    const stage = join(work, `${id}-stage`);
    mount = join(work, `${id}-mount`);
    mkdirSync(stage, { recursive: true });
    sh('ditto', [app, join(stage, `${NAME}.app`)]);
    sh('hdiutil', ['create', '-srcfolder', stage, '-volname', NAME, '-format', 'UDRO', '-ov', dmg]);
    mkdirSync(mount, { recursive: true });
    sh('hdiutil', ['attach', '-nobrowse', '-readonly', '-mountpoint', mount, dmg]);
    rmSync(app, { recursive: true, force: true });
    app = join(mount, `${NAME}.app`);
  }

  const env = {
    ...process.env,
    THOUGHTS_HOME: home,
    THOUGHTS_REHEARSAL: log,
    // The keychain test: the old version saves an entry, the new one reads it.
    ...(save ? { THOUGHTS_REHEARSAL_SAVE: OLD } : {}),
  };
  const binary = join(app, 'Contents', 'MacOS', 'thoughts');

  const started = Date.now();
  spawn(binary, [], { env, detached: true, stdio: 'ignore' }).unref();
  // The background service writes its version down while it runs and takes the note away when
  // it stops, so it is read while the copy is up.
  let service = null;
  const watch = setInterval(() => {
    try { service = JSON.parse(readFileSync(join(home, 'daemon.json'), 'utf8')).version ?? service; } catch { /* not running */ }
  }, 300);
  // Finished when a copy has said "done": the old one (nothing installed) or the new one (after the restart).
  const { seen, finished } = await waitFor(log, (l) => l.some((x) => x.step === 'done'), limitMs);
  await sleep(1500);
  clearInterval(watch);
  stopCopy(app);

  const last = (step, version) => seen.filter((x) => x.step === step && (!version || x.version === version)).at(-1);
  const result = {
    id, title, finished, seconds: Math.round((Date.now() - started) / 1000),
    installed: bundleVersion(app),
    check: last('check', OLD) ?? null,
    install: last('install', OLD) ?? null,
    newStarted: !!last('started', NEW),
    newCheck: last('check', NEW) ?? null,
    keychainSaved: last('keychain-save', OLD) ?? null,
    keychainRead: last('keychain-read', NEW)?.result ?? null,
    service,
    quarantine: sh('xattr', [app], { allowFail: true }).out.includes('com.apple.quarantine'),
    asked: { public: state.seen.filter((r) => r.path.startsWith('/public/')), signedIn: state.seen.filter((r) => r.signedIn).length },
  };
  result.ok = finished && result.installed === expectVersion;
  // The entry is not left for the scenarios that follow (or for anyone else).
  if (save) sh('security', ['delete-generic-password', '-s', KEYCHAIN, '-a', 'check'], { allowFail: true });
  if (mount) sh('hdiutil', ['detach', mount, '-force'], { allowFail: true });
  else rmSync(app, { recursive: true, force: true });
  return result;
}

/** The same update, clicked by a person: opens 0.0.90 with nothing automatic and waits for it to
 *  become 0.0.91. */
async function clickThrough(ctx) {
  const { work, state, builds } = ctx;
  const home = join(work, 'homes', 'by-hand');
  mkdirSync(home, { recursive: true });
  Object.assign(state, { public: 'manifest', tamper: false, seen: [] });
  const app = join(ctx.where, `${NAME}.app`);
  sh('ditto', [join(builds, OLD, `${NAME}.app`), app]);
  const env = { ...process.env, THOUGHTS_HOME: home };
  const binary = join(app, 'Contents', 'MacOS', 'thoughts');
  spawn(binary, [], { env, detached: true, stdio: 'ignore' }).unref();
  console.log(`\n"${NAME}" ${OLD} is open.`);
  console.log('In its menu choose "Check for Updates…", read what it offers, and install it.');
  console.log(`This waits until the copy has become ${NEW} (ten minutes at most), then closes it and cleans up.\n`);
  const until = Date.now() + 10 * 60_000;
  while (Date.now() < until && bundleVersion(app) !== NEW) await sleep(1000);
  const became = bundleVersion(app);
  if (became === NEW) {
    console.log(`The copy is now ${NEW}. Leaving it open for twenty seconds so the new version can be seen.`);
    await sleep(20_000);
  } else {
    console.log(`Still ${became || 'unknown'} after ten minutes: nothing was installed.`);
    process.exitCode = 1;
  }
  stopCopy(app);
}

// ---------------------------------------------------------------------------------- the report

function report(results, facts) {
  const by = Object.fromEntries(results.map((r) => [r.id, r]));
  const rows = [];
  const row = (what, verdict, detail = '') => rows.push(`| ${what} | ${verdict} | ${detail} |`);
  const yes = (b) => (b ? 'yes' : '**NO**');
  const why = (r) => (r.finished ? '' : 'it did not finish by itself: look for a question from macOS');

  if (by.public) {
    const r = by.public;
    row('An update from the public address installs and the new version starts', yes(r.ok && r.newStarted && r.asked.signedIn === 0), r.ok ? `0.0.90 became ${r.installed} in ${r.seconds} s, with no password and nothing to click` : `installed version is ${r.installed || 'unknown'}; ${r.install?.error ?? why(r)}`);
    row('The update check carries the release notes in both languages (shown only by copies older than 0.1.0)', yes(!!r.check?.notes && !!r.check?.notesAr), r.check?.notes ? `"${String(r.check.notes).slice(0, 40)}…"` : '');
    row('The new copy carries no quarantine mark', yes(!r.quarantine));
    row('The background service is the new version afterwards', r.service ? yes(r.service === NEW) : 'not seen', r.service ? `it reports ${r.service}` : 'the service had not started when the copy closed');
    const k = r.keychainRead;
    const kept = k?.value === `saved by ${OLD}`;
    row('A secret saved by the old version is read by the new one without a question',
      !r.keychainSaved?.ok ? 'not tested' : kept ? 'yes' : '**NO**',
      !r.keychainSaved?.ok ? 'the old version could not save one'
        : r.keychainSaved.inAFile ? 'the keychain was not used at all: the secret went to the fallback file'
        : kept ? `read in ${k.ms} ms` : k?.asked ? 'macOS asked the person (the read did not come back in 8 s)' : 'the new version got nothing back: macOS refused it or the entry is gone');
  }
  if (by.unpublished) {
    const r = by.unpublished;
    row('With nothing published at the public address, the check says so calmly and nothing is installed', yes(r.finished && r.check?.problem === 'unpublished' && r.installed === OLD), `said "${r.check?.problem ?? '?'}"`);
  }
  if (by.offline) {
    const r = by.offline;
    row('With no answer from the public address, the check says there is no connection and nothing is installed', yes(r.finished && r.check?.problem === 'offline' && r.installed === OLD), `said "${r.check?.problem ?? '?'}"`);
  }
  if (by.tampered) {
    const r = by.tampered;
    row('A download with one changed byte is refused and the old version stays', yes(r.finished && r.installed === OLD && !!r.install?.error), r.install?.error ? `"${String(r.install.error).slice(0, 70)}"` : why(r));
  }
  if (by.readonly) {
    const r = by.readonly;
    row('Opened from a disk image, the update fails cleanly', yes(r.finished && !!r.install?.error), r.install?.error ? `"${String(r.install.error).slice(0, 70)}"` : why(r));
  }
  return [
    `# Update rehearsal on this Mac`,
    '',
    `${new Date().toISOString().slice(0, 16).replace('T', ' ')} UTC · macOS ${facts.macos} · ${ARCH} · commit ${facts.commit}${facts.dirty ? ' + uncommitted changes' : ''}`,
    `The copies were signed as the released app is: ${facts.signature}. Installed in ${facts.where}.`,
    `Update files were signed with a throwaway key; the signature carried the version: ${facts.signedVersion ? 'yes' : '**NO**'}.`,
    '',
    '| Check | Result | Detail |',
    '|---|---|---|',
    ...rows,
    '',
    'Not covered: the microphone permission after an update, an update while a chat runs, a copy downloaded through a browser, an Intel Mac.',
  ].join('\n');
}

// ------------------------------------------------------------------------------------- main

const SCENARIOS = [
  { id: 'public', title: 'the public address answers', source: 'manifest', save: true, expectVersion: NEW },
  { id: 'unpublished', title: 'nothing published', source: 'notfound', expectVersion: OLD },
  { id: 'offline', title: 'no answer', source: 'silent', expectVersion: OLD },
  { id: 'tampered', title: 'one byte changed', source: 'manifest', tamper: true, expectVersion: OLD },
  { id: 'readonly', title: 'opened from a disk image', source: 'manifest', readOnly: true, expectVersion: OLD },
];

if (isMain(import.meta.url)) {
  await run(async () => {
    if (process.platform !== 'darwin') stop('The rehearsal is written for macOS.');
    const args = process.argv.slice(2);
    const value = (flag) => (args.includes(flag) ? args[args.indexOf(flag) + 1] : null);
    const keep = args.includes('--keep');
    const only = value('--only')?.split(',') ?? null;
    const where = (value('--where') ?? '/Applications').replace(/^~(?=\/|$)/, homedir());
    // Sign the copies with a certificate of this name from the keychain, to see what a stable
    // identity changes (a self-made certificate, or a Developer ID). Without it they are signed
    // as the released app is today.
    const identity = value('--identity');
    // `--by-hand`: nothing is clicked for you. The old version is
    // opened and waits for you to choose Check for Updates… in its menu, so the dialog (from 0.1.0 the
    // app's own words and a link to the release page, not the notes), the progress line and the restart can be seen as a person sees them.
    const byHand = args.includes('--by-hand');
    if (!existsSync(where)) stop(`${where} does not exist`);
    if (existsSync(join(where, `${NAME}.app`))) stop(`${join(where, `${NAME}.app`)} already exists: an earlier rehearsal did not clean up. Look at it, remove it, and run this again.`);

    // The real path: the temporary folder is reached through a link (/var), and the updater
    // refuses to work for an app whose own path goes through one.
    const work = realpathSync(mkdtempSync(join(tmpdir(), 'thoughts-rehearsal-')));
    const src = join(work, 'src');
    const builds = join(work, 'builds');
    for (const d of [src, builds, join(work, 'key'), join(work, 'logs'), join(work, 'homes')]) mkdirSync(d, { recursive: true });
    const state = { assets: [], seen: [], public: 'manifest', tamper: false };
    const server = await startFeed(state);
    state.base = `http://127.0.0.1:${server.address().port}`;
    const ctx = { work, builds, state, where };
    let text = '';
    try {
      // A throwaway key: never the real one, whose signature on a test build would be a valid update for good.
      const cli = join(ROOT, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
      sh(process.execPath, [cli, 'signer', 'generate', '--ci', '-w', join(work, 'key', 'rehearsal.key')]);
      const key = readFileSync(join(work, 'key', 'rehearsal.key'), 'utf8').trim();
      const pub = readFileSync(join(work, 'key', 'rehearsal.key.pub'), 'utf8').trim();

      copyTree(src);
      writeFileSync(join(src, 'src-tauri', 'rehearsal.conf.json'), JSON.stringify({
        productName: NAME,
        identifier: IDENTIFIER,
        bundle: { createUpdaterArtifacts: true, ...(identity ? { macOS: { signingIdentity: identity } } : {}) },
        plugins: {
          updater: { pubkey: pub, endpoints: [`${state.base}/public/latest.json`], requireSignedVersion: true, dangerousInsecureTransportProtocol: true },
          // Its own link scheme, so the real app keeps thoughts:// to itself.
          'deep-link': { desktop: { schemes: ['thoughts-rehearsal'] } },
        },
      }, null, 2));

      for (const version of [OLD, NEW]) {
        const made = buildOne(src, version, key);
        mkdirSync(join(builds, version), { recursive: true });
        sh('ditto', [join(made, `${NAME}.app`), join(builds, version, `${NAME}.app`)]);
        if (version === NEW) {
          for (const end of ['.app.tar.gz', '.app.tar.gz.sig']) cpSync(join(made, `${NAME}${end}`), join(builds, version, `${NAME}${end}`));
        }
      }
      const archive = readFileSync(join(builds, NEW, `${NAME}.app.tar.gz`));
      let signature = readFileSync(join(builds, NEW, `${NAME}.app.tar.gz.sig`), 'utf8').trim();
      const said = checkSignature({ publicKeyFile: pub, signatureFile: signature, data: archive });
      if (!said.ok) stop(`The update file the build made does not verify: ${said.why}`);
      const signedVersion = said.version === NEW;
      if (!signedVersion) console.log(`::warning::The build signed the update file ${said.version ? `for version ${said.version}` : 'without a version'}: installed copies would refuse it.`);

      const entry = (url) => ({ signature, url });
      const manifest = (url) => ({
        version: NEW, notes: 'Rehearsal notes: what changed.', notes_ar: 'ملاحظات التجربة: ما الذي تغيّر.', pub_date: new Date().toISOString(),
        platforms: { [`darwin-${ARCH}`]: entry(url), [`darwin-${ARCH}-app`]: entry(url) },
      });
      state.manifest = () => manifest(`${state.base}/public/files/${ARCHIVE}`);
      state.assets = [
        { name: ARCHIVE, bytes: archive },
        { name: `${ARCHIVE}.sig`, bytes: Buffer.from(signature) },
      ];

      if (byHand) {
        await clickThrough(ctx);
        return;
      }

      const results = [];
      for (const s of SCENARIOS.filter((x) => !only || only.includes(x.id))) {
        console.log(`Scenario: ${s.title} …`);
        results.push(await scenario(ctx, s));
        const r = results.at(-1);
        console.log(`  ${r.ok ? 'as expected' : 'NOT as expected'}: installed ${r.installed || '?'} after ${r.seconds} s${r.finished ? '' : ' (did not finish by itself)'}`);
      }
      const git = (...a) => sh('git', a, { allowFail: true }).out;
      const signedAs = sh('codesign', ['-dv', join(builds, NEW, `${NAME}.app`)], { allowFail: true });
      text = report(results, {
        macos: sh('sw_vers', ['-productVersion'], { allowFail: true }).out,
        commit: git('rev-parse', '--short', 'HEAD'),
        dirty: git('status', '--porcelain') !== '',
        signature: /Signature=adhoc/.test(signedAs.err + signedAs.out) ? 'ad-hoc, no Apple certificate' : ((signedAs.err + signedAs.out).match(/Authority=(.*)/)?.[1] ?? 'unknown'),
        signedVersion,
        where,
      });
      writeFileSync(join(work, 'REPORT.md'), `${text}\n`);
      writeFileSync(join(work, 'results.json'), JSON.stringify(results, null, 2));
      console.log(`\n${text}\n`);
      if (results.some((r) => !r.ok)) process.exitCode = 1;
    } finally {
      server.close();
      stopCopy(join(where, `${NAME}.app`));
      rmSync(join(where, `${NAME}.app`), { recursive: true, force: true });
      sh('security', ['delete-generic-password', '-s', KEYCHAIN, '-a', 'check'], { allowFail: true });
      if (keep) console.log(`Kept: ${work}`);
      else rmSync(work, { recursive: true, force: true });
    }
  });
}
