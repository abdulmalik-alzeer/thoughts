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
//   --skip-tests       do not run the tests first (TESTS below; the summary says so)
//   --include-others   build although commits since the last release were made by someone else:
//                      you have read them
//
// A collaborator on a personal repository can always push to `main`, and so can anyone whose
// change is merged, until rulesets for `main` and the `v*` tags are switched on (docs/RELEASING.md,
// "Repository settings worth switching on"). So before it builds, this command lists every commit
// since the last release that you did not make, and every commit on GitHub's `main` that this Mac
// does not have, and waits until you have read them. "You" is this Mac's git address
// (`git config user.email`), any address in RELEASE_OWN_EMAILS (comma-separated), and the address
// GitHub makes for the repository's owner (`<number>+<owner>@users.noreply.github.com`). With no
// earlier release tag (the first release) "since the last release" is the whole history, so only
// the commits of other identities are listed (Dependabot's, for instance), not every commit.
//
// Before it: set the version (scripts/set-version.mjs), write the entry in docs/CHANGELOG.md and
// docs/ar/CHANGELOG.md (it becomes the release page and the new version's Help › What's new; it is
// also written into latest.json, whose notes only copies older than 0.1.0 show), commit.
//
// It needs:
//   - the updater's private key in ~/.tauri/thoughts-updater.key, or TAURI_SIGNING_PRIVATE_KEY
//     holding the key or the path of its file;
//   - the key's password in TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or in a keychain (findPassword
//     below; the commands that set one up are in docs/RELEASING.md, "Keeping the updater key safe"):
//       * a separate keychain named by THOUGHTS_UPDATER_KEYCHAIN, which stays locked: this command
//         unlocks it (macOS asks for its password here), reads the password, and locks it again;
//       * an item "thoughts-updater", account "always-ask", made with `-T ""` in the login
//         keychain: macOS asks every time it is read;
//       * the item "thoughts-updater" made the old way (no `-T`). It still works, with a warning:
//         any program running as you can read it without being asked, with the very command
//         this script uses;
//   - the public key committed as src-tauri/updater.pub;
//   - `gh` signed in, for --draft and --publish.
//
// The key is handed to one step only, the one that packs and signs; the compile step, where the
// build code of every dependency runs, never sees it in its environment (scripts/tauri-ci.mjs, as
// in the pipeline). That only covers the environment: a password any program can read from the
// keychain, or a key file on the account you use every day, is open to that same build code, and
// to every assistant command run as you. Hence the warning above, and the advice in the docs to
// keep the key off the everyday account.
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
export const KEYCHAIN_SERVICE = 'thoughts-updater';
/** The account of the always-ask keychain item (made with `-T ""`, so macOS asks every time). */
export const ALWAYS_ASK_ACCOUNT = 'always-ask';
/** The account the item had when it was made the old way (docs before 0.1.0). */
const OLD_ACCOUNT = 'release';
const SAFETY_DOCS = 'docs/RELEASING.md, "Keeping the updater key safe"';
/** The workflows a push to `main` or of a tag would start. */
const WORKFLOWS = ['CI', 'Release', 'Advisories'];

/** `owner/name` from the address of the `origin` remote. */
export function repoFromRemote(url) {
  const m = String(url).trim().match(/github\.com[:/]([\w.-]+)\/([\w.-]+?)(?:\.git)?\/?$/);
  return m ? `${m[1]}/${m[2]}` : null;
}

/** The address every installed copy asks for updates, with no sign-in: the newest public
 *  release's `latest.json`. Before anything is published there it answers "not found". */
export const publicEndpoint = (repo) => `https://github.com/${repo}/releases/latest/download/latest.json`;

/** The private key: what TAURI_SIGNING_PRIVATE_KEY holds (the key, or the path of its file), or
 *  the file at the usual place. Null when there is none. */
export function findKey(env, { exists = existsSync, read = (p) => readFileSync(p, 'utf8'), fallback = DEFAULT_KEY } = {}) {
  const given = (env.TAURI_SIGNING_PRIVATE_KEY || '').trim();
  if (given) return exists(given) ? read(given).trim() : given;
  return exists(fallback) ? read(fallback).trim() : null;
}

/** Is `address` the one GitHub makes for the account `owner` (`<number>+<owner>@users.noreply.github.com`,
 *  or the older `<owner>@users.noreply.github.com`)? */
export function isOwnersNoreply(address, owner) {
  const login = String(owner ?? '').trim().toLowerCase();
  if (!/^[a-z0-9](?:[a-z0-9-]{0,38})$/.test(login)) return false;
  const m = /^(?:\d+\+)?([a-z0-9-]+)@users\.noreply\.github\.com$/.exec(String(address ?? '').trim().toLowerCase());
  return !!m && m[1] === login;
}

/** The commits in `log` that the person releasing did not make: the author or the committer is
 *  not one of `own` (addresses, in any letter case) nor GitHub's address for the account `owner`
 *  (the repository's owner). `log` is one commit a line, as
 *  `git log --format=%h%x1f%an%x1f%ae%x1f%ce%x1f%s` prints it. */
export function commitsByOthers(log, own, owner = null) {
  const mine = new Set([...own].map((e) => String(e).trim().toLowerCase()).filter(Boolean));
  const is = (address) => mine.has(String(address ?? '').toLowerCase()) || isOwnersNoreply(address, owner);
  return String(log ?? '')
    .split('\n')
    .filter(Boolean)
    .map((line) => {
      const [id, name, author, committer, ...subject] = line.split('\x1f');
      return { id, name, author, committer, subject: subject.join(' ') };
    })
    .filter((c) => !is(c.author) || !is(c.committer));
}

/** "since v1.2.2", or for the first release (no earlier tag) "in the whole history". */
export const sinceWords = (since) => (since ? `since ${since}` : 'in the whole history (this is the first release)');

/** Everything that must hold before a release is built. Returns the reasons it does not.
 *  `others`: the commits since the release `since` that someone else made (`commitsByOthers`);
 *  `behind`: how many commits GitHub's `main` has that this computer does not. */
export function preflightProblems({ branch, dirty, version, head, tagAt, key, password, passwordProblem = null, publicKey, keysMatch, changelog, changelogAr, others = [], since = null, includeOthers = false, behind = 0 }) {
  const problems = [];
  if (behind > 0) problems.push(`GitHub's main has ${behind} commit${behind === 1 ? '' : 's'} this Mac does not have: someone else pushed. Read ${behind === 1 ? 'it' : 'them'} before releasing (git log HEAD..origin/main), then bring ${behind === 1 ? 'it' : 'them'} in or leave ${behind === 1 ? 'it' : 'them'} out.`);
  if (others.length && !includeOthers) {
    const one = others.length === 1;
    const shown = others.slice(0, 20).map((c) => `    ${c.id}  ${c.name} <${c.author}>  ${c.subject}`).join('\n');
    const more = others.length > 20 ? `\n    … and ${others.length - 20} more` : '';
    problems.push(`${others.length} commit${one ? '' : 's'} ${sinceWords(since)} ${one ? 'was' : 'were'} not made by you:\n${shown}${more}\n  Read ${one ? 'it' : 'them'} (git show <id>), then run this again with --include-others.`);
  }
  if (branch !== 'main') problems.push(`Releases are made from main; this is "${branch}".`);
  if (dirty) problems.push('There are changes that are not committed. A release is built from a commit.');
  if (!/^\d+\.\d+\.\d+$/.test(version || '')) problems.push(`"${version}" is not a version like 1.2.3.`);
  if (tagAt && tagAt !== head) problems.push(`The tag v${version} already exists on another commit (${tagAt.slice(0, 7)}). Set a new version, or delete the tag if that version was never published.`);
  if (!key) problems.push(`The updater's private key was not found (looked in TAURI_SIGNING_PRIVATE_KEY and ${DEFAULT_KEY}).`);
  if (key && password === null) problems.push(passwordProblem || `The key's password was not found. Set TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or save it in a keychain as ${SAFETY_DOCS} describes.`);
  if (!publicKey) problems.push('src-tauri/updater.pub is missing. Commit the public key (the .pub file made with the private key): it is not secret, and every build reads it from there.');
  if (key && publicKey && password !== null && keysMatch === false) problems.push('The private key does not match src-tauri/updater.pub (or its password is wrong). A copy built with the wrong public key can never be updated.');
  if (!changelog) problems.push(`docs/CHANGELOG.md has no entry "## ${version}". It becomes the release page and this version's Help › What's new (and the update notes copies older than 0.1.0 show).`);
  if (!changelogAr) problems.push(`docs/ar/CHANGELOG.md has no entry "## ${version}". It becomes this version's Help › What's new in Arabic (and the update notes Arabic copies older than 0.1.0 show).`);
  return problems;
}

/** Whether node_modules holds exactly what package-lock.json asks for. A release built from a
 *  checkout whose packages are older than its lock file fails late, or builds with the wrong
 *  code, so this is checked first, without installing anything.
 *
 *  Every package the lock file lists must be in its folder at that version (read from the
 *  folder's own package.json, which is what Node loads), except optional ones, which npm skips
 *  on platforms they are not for (an optional package that is there must still be at that
 *  version). `hidden` is node_modules/.package-lock.json, npm's record of what it installed:
 *  a package in it the lock file no longer lists is left over. `installed(path)` answers the
 *  version in `<path>/package.json`, or null when there is none (the tests pass a fake).
 *  Returns the problem in plain words, or null. */
export function installProblem({ lock, hidden = null, installed }) {
  const wanted = lock && typeof lock.packages === 'object' ? lock.packages : null;
  if (!wanted) return 'package-lock.json lists no packages (lockfileVersion 2 or later is needed). Run: npm install, and commit the lock file.';
  const off = [];
  for (const [path, entry] of Object.entries(wanted)) {
    if (!path || !path.startsWith('node_modules/') || !entry) continue;
    const has = installed(path);
    if (entry.link) {
      if (has === null) off.push(`${path.slice(13)} is missing`);
      continue;
    }
    if (has === null) {
      if (!entry.optional) off.push(`${path.slice(13)} is missing`);
    } else if (entry.version && has !== entry.version) {
      off.push(`${path.slice(13)} is ${has}, the lock file says ${entry.version}`);
    }
  }
  for (const path of Object.keys(hidden?.packages ?? {})) {
    if (path.startsWith('node_modules/') && !(path in wanted)) off.push(`${path.slice(13)} is installed but no longer in the lock file`);
  }
  if (!off.length) return null;
  const shown = off.slice(0, 8).map((x) => `    ${x}`).join('\n');
  const more = off.length > 8 ? `\n    … and ${off.length - 8} more` : '';
  return `The installed packages (node_modules) do not match package-lock.json:\n${shown}${more}\n  Run: npm ci   (it installs exactly what the lock file lists), then run this again.`;
}

/** installProblem for this checkout. */
export function checkInstalled(root = ROOT) {
  const read = (file) => {
    try {
      return JSON.parse(readFileSync(file, 'utf8'));
    } catch {
      return null;
    }
  };
  const lock = read(join(root, 'package-lock.json'));
  if (!existsSync(join(root, 'node_modules'))) return 'There is no node_modules folder. Run: npm ci, then run this again.';
  const hidden = read(join(root, 'node_modules', '.package-lock.json'));
  return installProblem({ lock, hidden, installed: (path) => read(join(root, path, 'package.json'))?.version ?? null });
}

/** The tests run before a release is built (unless --skip-tests), in this order: the daemon, the
 *  desktop shell, the UI type checks, the UI's own tests, and the tests of these scripts. */
export const TESTS = [
  ['cargo', ['test', '-p', 'thoughtsd']],
  ['cargo', ['test', '-p', 'thoughts']],
  ['npm', ['run', 'check']],
  ['npx', ['vitest', 'run']],
  [process.execPath, ['--test', join(ROOT, 'scripts', 'release', 'release.test.mjs')]],
];

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

/** Runs /usr/bin/security. `interactive`: it may ask in this terminal (unlocking a keychain). */
function security(args, { interactive = false } = {}) {
  return sh('/usr/bin/security', args, { capture: !interactive, allowFail: true });
}

/** The path inside the quotes `security login-keychain` prints. */
export function keychainPath(out) {
  const m = String(out ?? '').match(/"([^"]+)"/);
  return m ? m[1] : String(out ?? '').trim() || null;
}

/** The updater key's password and where it came from. `run(args, { interactive })` stands for
 *  /usr/bin/security and answers `{ ok, out }` (the tests pass a fake). Returns
 *  `{ password, source, warnings, problem }`: `password` is null when there is none, and then
 *  `problem` may say why; `warnings` are printed but stop nothing.
 *
 *  In this order:
 *    1. TAURI_SIGNING_PRIVATE_KEY_PASSWORD, when set.
 *    2. THOUGHTS_UPDATER_KEYCHAIN: a separate keychain that stays locked. It is unlocked (macOS asks
 *       for its password in this terminal), the password is read, and it is locked again at once.
 *    3. The always-ask item (account "always-ask") in the login keychain. Whether it exists is
 *       looked up without reading the password, so a "Deny" stops here and never falls through to
 *       an older item.
 *    4. Any item "thoughts-updater": the old way. It works, with a warning, because unless it was
 *       made with `-T ""` any program running as you reads it without being asked. */
export function findPassword(env, { platform = process.platform, run = security } = {}) {
  const warnings = [];
  const result = (password, source, problem = null) => ({ password, source, warnings, problem });
  // An old item left behind after the move is still readable without a question.
  const oldItemLeft = () => {
    const login = run(['login-keychain']);
    const where = login.ok ? keychainPath(login.out) : null;
    if (where && run(['find-generic-password', '-s', KEYCHAIN_SERVICE, '-a', OLD_ACCOUNT, where]).ok) {
      warnings.push(`The old keychain item for the updater key's password (account "${OLD_ACCOUNT}") is still in your login keychain, where any program running as you can read it without being asked. Delete it (${SAFETY_DOCS}).`);
    }
  };

  if (env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD !== undefined) {
    warnings.push(`The updater key's password came from TAURI_SIGNING_PRIVATE_KEY_PASSWORD. Never keep it in a shell profile or any other file: every program running as you can read those. A keychain that asks is safer (${SAFETY_DOCS}).`);
    return result(env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD, 'environment');
  }
  if (platform !== 'darwin') return result(null, null);

  const keychain = (env.THOUGHTS_UPDATER_KEYCHAIN || '').trim();
  if (keychain) {
    if (!run(['unlock-keychain', keychain], { interactive: true }).ok) {
      return result(null, 'separate-keychain', `Could not unlock the keychain ${keychain} (THOUGHTS_UPDATER_KEYCHAIN): wrong password, or no such keychain.`);
    }
    let r;
    try {
      r = run(['find-generic-password', '-s', KEYCHAIN_SERVICE, '-w', keychain]);
    } finally {
      // Locked again before anything else runs, the build included.
      if (!run(['lock-keychain', keychain]).ok) warnings.push(`Could not lock ${keychain} again. Lock it yourself (${SAFETY_DOCS}).`);
    }
    if (!r.ok) return result(null, 'separate-keychain', `The keychain ${keychain} holds no item "${KEYCHAIN_SERVICE}", or macOS was not allowed to read it.`);
    oldItemLeft();
    return result(r.out, 'separate-keychain');
  }

  if (run(['find-generic-password', '-s', KEYCHAIN_SERVICE, '-a', ALWAYS_ASK_ACCOUNT]).ok) {
    const r = run(['find-generic-password', '-s', KEYCHAIN_SERVICE, '-a', ALWAYS_ASK_ACCOUNT, '-w']);
    if (!r.ok) return result(null, 'always-ask', `macOS was not allowed to read the updater key's password. Run this again and choose Allow (never Always Allow: that would let any program read it without asking).`);
    oldItemLeft();
    return result(r.out, 'always-ask');
  }

  const r = run(['find-generic-password', '-s', KEYCHAIN_SERVICE, '-w']);
  if (!r.ok) return result(null, null);
  warnings.push(`The updater key's password is in a keychain item that any program running as you can read without being asked (with the same command this script uses): a dependency's build code, or a command an assistant runs. Move it to an always-ask item or to a separate keychain that stays locked, and keep the key off the account you use every day (${SAFETY_DOCS}).`);
  return result(r.out, 'login-keychain');
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

function facts({ withKey = true } = {}) {
  const version = readVersion();
  const changelog = (file) => (existsSync(join(ROOT, file)) ? readFileSync(join(ROOT, file), 'utf8') : '');
  // --publish signs nothing, so it does not ask for the key's password.
  const key = withKey ? findKey(process.env) : null;
  const found = key ? findPassword(process.env) : { password: null, warnings: [], problem: null };
  for (const w of found.warnings) console.warn(`Warning: ${w}`);
  const password = found.password;
  const publicKey = existsSync(PUBLIC_KEY) ? readFileSync(PUBLIC_KEY, 'utf8').trim() : '';
  const tag = git('rev-parse', '-q', '--verify', `refs/tags/v${version}^{commit}`);
  // Whose commits go into this release: everything since the release before it.
  const own = [git('config', 'user.email').out, ...(process.env.RELEASE_OWN_EMAILS || '').split(',')];
  // No earlier tag (the first release): the whole history, of which only other identities are listed.
  const since = git('tag', '--merged', 'HEAD', '--sort=-v:refname', '--list', 'v*').out.split('\n').find((t) => t && t !== `v${version}`) ?? null;
  const log = git('log', '--format=%h%x1f%an%x1f%ae%x1f%ce%x1f%s', since ? `${since}..HEAD` : 'HEAD').out;
  const repo = repoFromRemote(git('remote', 'get-url', 'origin').out);
  // What GitHub has that this Mac does not. Best effort: with no connection the last known state counts.
  git('fetch', '--quiet', 'origin', 'main');
  return {
    since,
    others: commitsByOthers(log, own, repo?.split('/')[0]),
    behind: Number(git('rev-list', '--count', 'HEAD..origin/main').out) || 0,
    version,
    tag: `v${version}`,
    branch: git('branch', '--show-current').out,
    dirty: git('status', '--porcelain').out !== '',
    head: git('rev-parse', 'HEAD').out,
    tagAt: tag.ok ? tag.out : null,
    repo,
    key,
    password,
    passwordProblem: found.problem,
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
    const unknown = [...args].filter((a) => !['--draft', '--publish', '--skip-tests', '--include-others'].includes(a));
    if (unknown.length || (args.has('--draft') && args.has('--publish'))) stop('usage: local.mjs [--draft | --publish] [--skip-tests] [--include-others]');
    if (process.platform !== 'darwin') stop('This command builds the macOS release, so it runs on a Mac.');
    // Before anything else: the key check below already runs the Tauri CLI from node_modules.
    if (!args.has('--publish')) {
      const stale = checkInstalled();
      if (stale) stop(stale);
    }

    const f = facts({ withKey: !args.has('--publish') });
    if (!f.repo) stop('The origin remote is not a GitHub repository.');

    if (args.has('--publish')) {
      const url = await makePublic(f);
      console.log(`\n${f.tag} is public: ${url}\nEvery copy that checks for updates is offered it from now on.`);
      return;
    }

    const problems = preflightProblems({ ...f, includeOthers: args.has('--include-others') });
    if (problems.length) {
      console.error(`Not ready to release ${f.tag}:\n- ${problems.join('\n- ')}\n`);
      stop(`${problems.length === 1 ? 'One thing' : `${problems.length} things`} to put right before ${f.tag} can be released (listed above)`);
    }

    let built = args.has('--draft') ? builtEarlier(f) : null;
    if (built) {
      console.log(`Using the files this command built for ${f.tag} (${f.head.slice(0, 7)}) earlier.`);
    } else {
      // As the pipeline's preflight does: the list of what earlier releases shipped in the
      // starter kit must be current, or libraries made by older versions are asked for trust again.
      sh(process.execPath, [join(ROOT, 'scripts', 'kit-history.mjs'), '--check', '--before', f.version]);
      if (!args.has('--skip-tests')) for (const [cmd, cmdArgs] of TESTS) sh(cmd, cmdArgs);
      built = build(f);
    }
    console.log(`\nBuilt and checked ${f.tag} (${f.head.slice(0, 7)})${args.has('--skip-tests') ? ' — THE TESTS WERE SKIPPED' : ''}:`);
    for (const file of built.release.files) console.log(`  ${file.name}  ${megabytes(file.bytes)}`);
    console.log('  Every signed file verifies against src-tauri/updater.pub, for this version.');
    console.log(f.others.length
      ? `  It includes ${f.others.length} commit${f.others.length === 1 ? '' : 's'} ${sinceWords(f.since)} not made by you, which you said you have read (--include-others).`
      : `  Every commit ${sinceWords(f.since)} is yours.`);

    if (!args.has('--draft')) {
      console.log(`\nNothing was sent anywhere. The files are in release-out/ and release-meta/.\nNext: node scripts/release/local.mjs --draft`);
      return;
    }
    const result = await draft(f, built);
    console.log(`\nDraft release ${f.tag} holds ${result.files.length} files: ${result.url}\nNobody is offered a draft. Try the .dmg, then: node scripts/release/local.mjs --publish`);
  });
}
