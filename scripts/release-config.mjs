#!/usr/bin/env node
// Used by .github/workflows/release.yml. Writes src-tauri/release.conf.json, an extra Tauri
// config merged into the build with `--config`. It only adds what the environment provides:
//
//   UPDATER_PUBKEY + UPDATER_ENDPOINT   -> turn the in-app updater on and build signed
//                                          update artifacts (needs TAURI_SIGNING_PRIVATE_KEY)
//   WINDOWS_CERTIFICATE_THUMBPRINT      -> sign the Windows installer with that certificate
//
// With none of them set it writes `{}`, and the build is the same as a local one.
// No secret is read or written here: the public key and the thumbprint are not secret.
//
// The workflow calls it twice. Before compiling, for the updater settings (they are compiled
// into the app). Then, after compiling, with `--add-windows-signing`: the Windows certificate is
// only imported once every dependency's build code has finished running, so only then is there a
// thumbprint to add to the file the bundle step reads.
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const env = (k) => (process.env[k] || '').trim();
const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri', 'release.conf.json');
const addWindowsSigning = process.argv.includes('--add-windows-signing');

let conf = {};
if (addWindowsSigning) {
  if (!existsSync(out)) {
    console.error('release.conf.json does not exist yet: run this script without --add-windows-signing first.');
    process.exit(1);
  }
  conf = JSON.parse(readFileSync(out, 'utf8'));
} else if (env('UPDATER_PUBKEY') && env('UPDATER_ENDPOINT')) {
  conf.bundle = { createUpdaterArtifacts: true };
  conf.plugins = { updater: { pubkey: env('UPDATER_PUBKEY'), endpoints: [env('UPDATER_ENDPOINT')] } };
}
if (env('WINDOWS_CERTIFICATE_THUMBPRINT')) {
  conf.bundle = {
    ...conf.bundle,
    windows: {
      certificateThumbprint: env('WINDOWS_CERTIFICATE_THUMBPRINT'),
      digestAlgorithm: 'sha256',
      timestampUrl: env('WINDOWS_TIMESTAMP_URL') || 'http://timestamp.digicert.com',
    },
  };
}

writeFileSync(out, JSON.stringify(conf, null, 2) + '\n');
console.log(`release.conf.json: updater ${conf.plugins ? 'on' : 'off'}, Windows signing ${conf.bundle?.windows ? 'on' : 'off'}`);
