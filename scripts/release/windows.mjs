// The Windows installer of a release made on this Mac (scripts/release/local.mjs).
//
// A Mac cannot build the Windows installer, and the updater's private key never leaves this Mac.
// So the two halves meet here:
//
//   1. GitHub builds the installer: the Release workflow, started by hand for the release's own
//      commit (a "rehearsal" of the Windows platform). That run has no secret. The app in it is
//      built with the updater switched on (the public key is committed), but the installer leaves
//      the run without an update signature (`signLater` in its build-info-windows.json). The run
//      also installs and starts it on a fresh Windows machine (job "smoke-windows").
//   2. This Mac takes the installer from that run, checks it is the file that run recorded, built
//      from this commit and for this version, and signs it with the updater key. From then on it
//      is an ordinary release file: assemble.mjs lists it in latest.json, and installed Windows
//      copies accept it as an update.
//
// What is trusted: GitHub's runner built the installer from the commit it says. The same trust a
// release made wholly by the pipeline would need, minus handing GitHub the key.
//
// Everything here is plain logic over values; the parts that start `gh` and the Tauri CLI are in
// local.mjs.
import { copyFileSync, existsSync, lstatSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { SAFE_NAME, sha256, stop } from './lib.mjs';

export const WORKFLOW_PATH = '.github/workflows/release.yml';
export const ARTIFACT = 'installers-windows';
const REPORT = 'build-info-windows.json';

/** Of the runs of the Release workflow that were started by hand for `commit`, the one to use:
 *  the newest that passed, else the newest still going, else none (start a new one).
 *  `runs` is `gh run list --json databaseId,status,conclusion,headSha,event,createdAt`. */
export function pickRun(runs, commit) {
  const ours = (Array.isArray(runs) ? runs : [])
    .filter((r) => r.headSha === commit && r.event === 'workflow_dispatch')
    .sort((a, b) => (a.createdAt < b.createdAt ? 1 : -1));
  return ours.find((r) => r.status === 'completed' && r.conclusion === 'success') ?? ours.find((r) => r.status !== 'completed') ?? null;
}

/** Why the installer of this run must not be signed, or null. `run` is GitHub's own record of it
 *  (`GET /repos/{repo}/actions/runs/{id}`), never anything the run wrote itself. */
export function runProblem(run, { repo, commit }) {
  if (!run || typeof run !== 'object') return 'GitHub has no such run';
  if (String(run.repository?.full_name ?? '').toLowerCase() !== String(repo).toLowerCase()) return `it belongs to ${run.repository?.full_name ?? 'another repository'}, not ${repo}`;
  if (run.path !== WORKFLOW_PATH) return `it is a run of ${run.path}, not of ${WORKFLOW_PATH}`;
  if (run.event !== 'workflow_dispatch') return `it was started by "${run.event}", not by hand`;
  if (run.head_sha !== commit) return `it built ${String(run.head_sha).slice(0, 7)}, and this release is ${commit.slice(0, 7)}`;
  if (run.status !== 'completed') return 'it has not finished';
  if (run.conclusion !== 'success') return `it ended as "${run.conclusion}" (the build, or installing and starting the app on Windows, failed)`;
  return null;
}

/**
 * Takes the Windows installer out of `from` (the downloaded workflow artifact), signs it and puts
 * it in `out` beside the macOS files, with a build-info-windows.json that says the updater is on.
 *
 * `sign(path)` must write `<path>.sig` (the Tauri CLI's `signer sign --app-version`). Stops when
 * the folder holds anything but the one installer and its report, when the report is for another
 * version or commit, when the file is not the one the report describes, or when the build did not
 * have the updater compiled in (a copy without it could never be updated, signed or not).
 */
export function adopt({ from, out, version, commit, sign }) {
  if (!existsSync(from)) stop(`${from} does not exist: the Windows installer was not downloaded`);
  const found = readdirSync(from).sort();
  for (const name of found) if (!lstatSync(join(from, name)).isFile()) stop(`${name} in the Windows build is not a plain file`);
  let info;
  try {
    info = JSON.parse(readFileSync(join(from, REPORT), 'utf8'));
  } catch {
    stop(`The Windows build has no readable ${REPORT}`);
  }
  if (info.schema !== 1 || info.platform !== 'windows') stop(`${REPORT} is not a Windows build report`);
  if (info.version !== version) stop(`The Windows installer is version ${info.version}, and this release is ${version}`);
  if (info.commit !== commit) stop(`The Windows installer was built from ${String(info.commit).slice(0, 7)}, and this release is ${commit.slice(0, 7)}`);
  if (info.updater === true) stop('The Windows installer already carries an update signature made somewhere else; this command only signs unsigned ones');
  if (info.signLater !== true) stop('The Windows installer was built without the updater (no public key in that build). An installed copy of it could never be updated: do not release it.');
  if (!Array.isArray(info.files) || info.files.length !== 1 || info.files[0].kind !== 'nsis') stop(`${REPORT} should list exactly one installer`);
  const file = info.files[0];
  if (!SAFE_NAME.test(file.name) || !file.name.endsWith('-setup.exe') || !file.name.includes(`_${version}_`)) stop(`"${file.name}" is not the installer's expected name for ${version}`);
  const expected = [REPORT, file.name].sort();
  if (found.join('\n') !== expected.join('\n')) stop(`The Windows build should hold exactly ${expected.join(' and ')}; found ${found.join(', ') || 'nothing'}`);
  const given = join(from, file.name);
  if (statSync(given).size !== file.bytes || sha256(given) !== file.sha256) stop(`${file.name} is not the file the Windows build recorded (size or checksum differ)`);

  const exe = join(out, file.name);
  const sig = `${exe}.sig`;
  copyFileSync(given, exe);
  sign(exe);
  if (!existsSync(sig) || !readFileSync(sig, 'utf8').trim()) stop(`${file.name} could not be signed (no ${file.name}.sig was written)`);
  if (sha256(exe) !== file.sha256) stop(`${file.name} changed while it was being signed`);

  const files = [
    { ...file, signature: `${file.name}.sig` },
    { name: `${file.name}.sig`, kind: 'signature', bytes: statSync(sig).size, sha256: sha256(sig), signature: null },
  ];
  const signedInfo = { schema: 1, platform: 'windows', version, commit, updater: true, signed: info.signed ?? { windows: false }, files };
  writeFileSync(join(out, REPORT), JSON.stringify(signedInfo, null, 2) + '\n');
  return signedInfo;
}
