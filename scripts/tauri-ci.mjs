#!/usr/bin/env node
// Used by .github/workflows/release.yml as tauri-action's `tauriScript`: runs the Tauri CLI with
// the signing secrets the workflow passed to that ONE step, after dropping the ones that are not
// usable. Tauri treats a variable that is set but empty as "sign with this", and fails — so a
// repository with no secrets (or only some) must not hand it empty values.
//
//   macOS signing        needs APPLE_CERTIFICATE + APPLE_SIGNING_IDENTITY (else all APPLE_* dropped)
//   macOS notarisation   needs APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID (else those three dropped)
//   updater signing      needs TAURI_SIGNING_PRIVATE_KEY (else it and its password dropped)
//
// Nothing secret is printed or written anywhere.
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';

const env = { ...process.env };
const has = (k) => (env[k] || '').trim() !== '';
const drop = (...keys) => keys.forEach((k) => delete env[k]);

const notarise = ['APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID'];
const sign = has('APPLE_CERTIFICATE') && has('APPLE_SIGNING_IDENTITY');
if (!sign) drop('APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY', ...notarise);
else if (!notarise.every(has)) drop(...notarise);
const updater = has('TAURI_SIGNING_PRIVATE_KEY');
if (!updater) drop('TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD');

if (process.platform === 'darwin') {
  console.log(`macOS signing: ${sign ? 'on' : 'off (unsigned build)'}; notarisation: ${sign && notarise.every(has) ? 'on' : 'off'}`);
}
console.log(`Updater signing key: ${updater ? 'present' : 'absent'}`);

const cli = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
const r = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], { stdio: 'inherit', env });
process.exit(r.status ?? 1);
