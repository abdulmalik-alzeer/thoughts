#!/usr/bin/env node
// Used by .github/workflows/release.yml: runs the Tauri CLI for the two halves of a release
// build, and decides what each half may see in its environment.
//
//   node scripts/tauri-ci.mjs build --no-bundle …   COMPILE. This is where the build scripts and
//       procedural macros of every Rust dependency, and every npm package the UI build loads,
//       run. It gets no signing material and no GitHub token: whatever the workflow passed is
//       removed here before the CLI starts (and reported, because it should not have been passed).
//
//   node scripts/tauri-ci.mjs bundle …              PACKAGE AND SIGN. Runs no dependency build
//       code: it packs the compiled app with the system's tools and signs it. It receives the
//       signing secrets the workflow gave to that ONE step, after dropping the ones that are not
//       usable. Tauri treats a variable that is set but empty as "sign with this", and fails — so
//       a repository with no secrets (or only some) must not hand it empty values.
//
//   macOS signing        needs APPLE_CERTIFICATE + APPLE_SIGNING_IDENTITY (else all APPLE_* dropped)
//   macOS notarisation   needs APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID (else those three dropped)
//   updater signing      needs TAURI_SIGNING_PRIVATE_KEY (else it and its password dropped)
//
// Nothing secret is printed or written anywhere.
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';

const args = process.argv.slice(2);
const env = { ...process.env };
const has = (k) => (env[k] || '').trim() !== '';
const drop = (...keys) => keys.forEach((k) => delete env[k]);

const notarise = ['APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID'];
const apple = ['APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY', ...notarise, 'APPLE_API_KEY', 'APPLE_API_ISSUER', 'APPLE_API_KEY_PATH'];
const updaterKey = ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD'];
const windows = ['WINDOWS_CERTIFICATE', 'WINDOWS_CERTIFICATE_PASSWORD'];
const tokens = ['GITHUB_TOKEN', 'GH_TOKEN'];

// The Tauri CLI never needs a GitHub token (the tools it downloads are public).
drop(...tokens);

const compileOnly = args[0] === 'build' && args.includes('--no-bundle');
if (compileOnly) {
  const present = [...apple, ...updaterKey, ...windows].filter(has);
  if (present.length) {
    console.log(`::warning::The compile step was given ${present.join(', ')}. Signing material belongs to the bundle step only; it was removed before compiling.`);
  }
  drop(...apple, ...updaterKey, ...windows);
  console.log('Compile: no signing material and no GitHub token in the environment');
} else {
  const sign = has('APPLE_CERTIFICATE') && has('APPLE_SIGNING_IDENTITY');
  if (!sign) drop('APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY', ...notarise);
  else if (!notarise.every(has)) drop(...notarise);
  const updater = has('TAURI_SIGNING_PRIVATE_KEY');
  if (!updater) drop(...updaterKey);
  drop(...windows); // the Windows certificate is used through the certificate store, by its thumbprint

  if (process.platform === 'darwin') {
    console.log(`macOS signing: ${sign ? 'on' : 'off (unsigned build)'}; notarisation: ${sign && notarise.every(has) ? 'on' : 'off'}`);
    // On CI the bundler would skip arranging the disk image's window (icon positions). Keep the
    // arranged window, as every release so far has had it (tauri-action set the same default).
    if (args[0] === 'bundle') env.TAURI_BUNDLER_DMG_IGNORE_CI ??= 'true';
  }
  console.log(`Updater signing key: ${updater ? 'present' : 'absent'}`);
}

const cli = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
const r = spawnSync(process.execPath, [cli, ...args], { stdio: 'inherit', env });
process.exit(r.status ?? 1);
