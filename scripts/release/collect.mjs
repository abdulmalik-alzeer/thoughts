#!/usr/bin/env node
// Used by .github/workflows/release.yml, in each platform job after `tauri bundle`. Gathers
// exactly the files this platform contributes to a release into one flat folder, under the
// names they will have on the release page, and writes build-info-<platform>.json describing
// them (size, sha256, which file is an updater signature for which installer).
//
// It fails when an expected installer is missing, when there is more than one candidate, or
// when the updater is on and a signature is missing — a release with a hole in it is never
// uploaded as an artifact, so it can never be attached to a release.
//
//   PLATFORM  macos | windows | linux        VERSION  the app version (no "v")
//   TARGET    the --target the build used, or empty
//   UPDATER   "true" when signed update files were built
//   OUT       output folder (default: release-out)
//
// File names follow what tauri-action produced for 0.0.8 and earlier, so the release page and
// any saved download link keep working.
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { isMain, megabytes, PLATFORMS, ROOT, run, SAFE_NAME, sha256, stop } from './lib.mjs';

/** The one file in `dir` whose name ends with `suffix`. */
function only(dir, suffix, what) {
  const names = existsSync(dir)
    ? readdirSync(dir, { withFileTypes: true }).filter((e) => e.isFile() && e.name.endsWith(suffix)).map((e) => e.name)
    : [];
  if (names.length !== 1) {
    const seen = existsSync(dir) ? readdirSync(dir).join(', ') || '(empty)' : '(folder does not exist)';
    stop(`Expected exactly one ${what} (*${suffix}) in ${dir}, found ${names.length}. The folder holds: ${seen}`);
  }
  return names[0];
}

/** The processor an installer is for, read from the name Tauri gave it, in the updater's terms. */
const ARCH = { universal: 'universal', x64: 'x86_64', amd64: 'x86_64', x86_64: 'x86_64', arm64: 'aarch64', aarch64: 'aarch64', x86: 'i686', i386: 'i686', i686: 'i686', armhf: 'armv7', armv7: 'armv7' };
function archToken(name) {
  const m = name.match(/_(universal|x86_64|x64|amd64|arm64|aarch64|x86|i386|i686|armhf|armv7)(?=[.-])/);
  if (!m) stop(`Cannot tell which processor ${name} is for`);
  return m[1];
}

export function collect({ platform, version, target = '', updater = false, root = ROOT, out, commit = '', signed = {}, tar = defaultTar }) {
  if (!PLATFORMS.some((p) => p.id === platform)) stop(`Unknown platform "${platform}"`);
  if (!version) stop('VERSION is not set');
  const bundle = join(root, 'target', target, 'release', 'bundle');
  const product = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')).productName;
  if (!product || !SAFE_NAME.test(product)) stop(`productName "${product}" cannot be used in a file name`);
  mkdirSync(out, { recursive: true });

  const files = [];
  // Copies one installer into `out`, with its updater signature when the updater is on and
  // this kind of file is one the updater installs (`updates`).
  const take = (dir, source, kind, { name = source, updates = true } = {}) => {
    if (!SAFE_NAME.test(name)) stop(`"${name}" is not a safe release file name`);
    if (!name.includes(`_${version}_`)) stop(`${name} does not carry version ${version}: the build and the tag disagree`);
    copyFileSync(join(dir, source), join(out, name));
    const entry = { name, kind, arch: ARCH[archToken(name)], bytes: statSync(join(out, name)).size, sha256: sha256(join(out, name)), signature: null };
    files.push(entry);
    const sig = join(dir, `${source}.sig`);
    if (!updates) return;
    if (updater) {
      if (!existsSync(sig)) stop(`The updater is on but ${source}.sig was not produced: this installer could not be offered as an update`);
      if (!readFileSync(sig, 'utf8').trim()) stop(`${source}.sig is empty`);
      copyFileSync(sig, join(out, `${name}.sig`));
      entry.signature = `${name}.sig`;
      files.push({ name: `${name}.sig`, kind: 'signature', bytes: statSync(join(out, `${name}.sig`)).size, sha256: sha256(join(out, `${name}.sig`)), signature: null });
    } else if (existsSync(sig)) {
      console.log(`::warning::${source}.sig exists although the updater is off; it is left out.`);
    }
  };

  if (platform === 'macos') {
    const dmgDir = join(bundle, 'dmg');
    const dmg = only(dmgDir, '.dmg', 'disk image');
    take(dmgDir, dmg, 'dmg', { updates: false }); // people install from it; updates use the archive

    // The app itself, as an archive. With the updater on, Tauri made and signed it; otherwise it
    // is packed here the way tauri-action did, so every release carries the same set of files.
    const appDir = join(bundle, 'macos');
    if (!existsSync(join(appDir, `${product}.app`))) stop(`${product}.app was not produced in ${appDir}`);
    const archive = `${product}.app.tar.gz`;
    if (!existsSync(join(appDir, archive))) {
      if (updater) stop(`The updater is on but ${archive} was not produced`);
      tar(appDir, archive, `${product}.app`);
    }
    // Tauri names it after the app only; add version and processor, as in the disk image's name.
    take(appDir, archive, 'app', { name: `${product}_${version}_${archToken(dmg)}.app.tar.gz` });
  } else if (platform === 'windows') {
    const dir = join(bundle, 'nsis');
    take(dir, only(dir, '-setup.exe', 'Windows installer'), 'nsis');
  } else {
    const deb = join(bundle, 'deb');
    take(deb, only(deb, '.deb', 'Debian package'), 'deb');
    const appimage = join(bundle, 'appimage');
    take(appimage, only(appimage, '.AppImage', 'AppImage'), 'appimage');
  }

  const info = { schema: 1, platform, version, commit, updater, signed, files };
  writeFileSync(join(out, `build-info-${platform}.json`), JSON.stringify(info, null, 2) + '\n');
  return info;
}

function defaultTar(dir, archive, entry) {
  const r = spawnSync('tar', ['czf', archive, '-C', dir, entry], { cwd: dir, stdio: 'inherit' });
  if (r.status !== 0) stop(`tar could not pack ${entry}`);
}

if (isMain(import.meta.url)) {
  await run(async () => {
    const flag = (name) => process.env[name] === 'true';
    const platform = process.env.PLATFORM;
    // Each build reports the signing of its own platform only (Linux packages are not signed).
    const signed =
      platform === 'macos'
        ? { macos: flag('MACOS_SIGNED'), notarized: flag('MACOS_SIGNED') && flag('MACOS_NOTARIZED') }
        : platform === 'windows'
          ? { windows: flag('WINDOWS_SIGNED') }
          : {};
    const info = collect({
      platform,
      version: process.env.VERSION,
      target: process.env.TARGET || '',
      updater: flag('UPDATER'),
      out: join(ROOT, process.env.OUT || 'release-out'),
      commit: process.env.GITHUB_SHA || '',
      signed,
    });
    for (const f of info.files) console.log(`${f.name}  ${megabytes(f.bytes)}  sha256:${f.sha256}`);
  });
}
