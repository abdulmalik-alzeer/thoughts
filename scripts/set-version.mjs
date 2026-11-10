#!/usr/bin/env node
// Set the app version everywhere it is written down.
//
//   node scripts/set-version.mjs 0.0.3
//
// The version lives in two files: the root Cargo.toml ([workspace.package], which both
// Rust crates inherit and which Tauri reads because tauri.conf.json has no "version" key)
// and package.json (plus package-lock.json when present). Run `cargo check -p thoughts`
// afterwards so Cargo.lock follows.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version || '')) {
  console.error('Usage: node scripts/set-version.mjs <major.minor.patch>');
  process.exit(1);
}
const root = join(dirname(fileURLToPath(import.meta.url)), '..');

const cargoPath = join(root, 'Cargo.toml');
const cargo = readFileSync(cargoPath, 'utf8');
const re = /(\[workspace\.package\][^\[]*?\nversion\s*=\s*")[^"]*(")/;
if (!re.test(cargo)) {
  console.error('Could not find [workspace.package] version in Cargo.toml');
  process.exit(1);
}
writeFileSync(cargoPath, cargo.replace(re, `$1${version}$2`));

const pkgPath = join(root, 'package.json');
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));
pkg.version = version;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');

const lockPath = join(root, 'package-lock.json');
if (existsSync(lockPath)) {
  const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
  lock.version = version;
  if (lock.packages?.['']) lock.packages[''].version = version;
  writeFileSync(lockPath, JSON.stringify(lock, null, 2) + '\n');
}

const conf = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
if ('version' in conf) {
  console.error('src-tauri/tauri.conf.json has a "version" key again: remove it so Tauri follows Cargo.toml.');
  process.exit(1);
}
console.log(`Version set to ${version}. Now run: cargo check -p thoughts   (updates Cargo.lock)`);
