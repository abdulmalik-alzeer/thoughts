#!/usr/bin/env node
// Used by .github/workflows/release.yml and by scripts/release/local.mjs. Writes
// src-tauri/release.conf.json, an extra Tauri config merged into the build with `--config`. It
// only adds what it is given:
//
//   the updater's public key + UPDATER_ENDPOINT  -> turn the in-app updater on and build signed
//                                                   update files (needs TAURI_SIGNING_PRIVATE_KEY)
//   WINDOWS_CERTIFICATE_THUMBPRINT               -> sign the Windows installer with that certificate
//
// With none of them it writes `{}`, and the build is the same as a local one.
//
// The public key is read from src-tauri/updater.pub, the committed file, so that a build made on
// this computer and one made in CI cannot disagree about it: a copy built with the wrong public
// key can never be updated. UPDATER_PUBKEY (the CI variable from before the file existed) is
// still accepted, and the build is refused when the two differ. UPDATER_ENDPOINT may hold several
// addresses, separated by spaces or commas; the app asks them in that order.
//
// Every updater build sets `requireSignedVersion`: an installed copy then refuses an update whose
// signature was not made for the version the address announces, so an old, genuinely signed
// release cannot be served again under a new number.
//
// No secret is read or written here: the public key and the thumbprint are not secret.
//
// The workflow calls it twice. Before compiling, for the updater settings (they are compiled
// into the app). Then, after compiling, with `--add-windows-signing`: the Windows certificate is
// only imported once every dependency's build code has finished running, so only then is there a
// thumbprint to add to the file the bundle step reads.
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { isMain } from './release/lib.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const OUT = join(here, '..', 'src-tauri', 'release.conf.json');
/** The updater's public key. Not secret; committed. */
export const PUBLIC_KEY_FILE = join(here, '..', 'src-tauri', 'updater.pub');

/** The config for these settings. `existing` is the file written by the first call. */
export function releaseConfig({ env = {}, committedKey = '', existing = null, addWindowsSigning = false } = {}) {
  const get = (k) => (env[k] || '').trim();
  let conf = {};
  if (addWindowsSigning) {
    if (!existing) throw new Error('release.conf.json does not exist yet: run this script without --add-windows-signing first.');
    conf = existing;
  } else {
    const given = get('UPDATER_PUBKEY');
    const committed = String(committedKey).trim();
    if (given && committed && given !== committed) {
      throw new Error('UPDATER_PUBKEY and src-tauri/updater.pub hold different keys. A copy built with the wrong public key can never be updated: make them the same, or drop the variable.');
    }
    const pubkey = committed || given;
    const endpoints = get('UPDATER_ENDPOINT').split(/[\s,]+/).filter(Boolean);
    if (pubkey && endpoints.length) {
      conf.bundle = { createUpdaterArtifacts: true };
      conf.plugins = { updater: { pubkey, endpoints, requireSignedVersion: true } };
    }
  }
  if (get('WINDOWS_CERTIFICATE_THUMBPRINT')) {
    conf.bundle = {
      ...conf.bundle,
      windows: {
        certificateThumbprint: get('WINDOWS_CERTIFICATE_THUMBPRINT'),
        digestAlgorithm: 'sha256',
        timestampUrl: get('WINDOWS_TIMESTAMP_URL') || 'http://timestamp.digicert.com',
      },
    };
  }
  return conf;
}

if (isMain(import.meta.url)) {
  try {
    const conf = releaseConfig({
      env: process.env,
      committedKey: existsSync(PUBLIC_KEY_FILE) ? readFileSync(PUBLIC_KEY_FILE, 'utf8') : '',
      existing: existsSync(OUT) ? JSON.parse(readFileSync(OUT, 'utf8')) : null,
      addWindowsSigning: process.argv.includes('--add-windows-signing'),
    });
    writeFileSync(OUT, JSON.stringify(conf, null, 2) + '\n');
    console.log(`release.conf.json: updater ${conf.plugins ? 'on' : 'off'}, Windows signing ${conf.bundle?.windows ? 'on' : 'off'}`);
  } catch (e) {
    console.error(String(e?.message ?? e));
    process.exit(1);
  }
}
