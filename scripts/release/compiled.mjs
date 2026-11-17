#!/usr/bin/env node
// Hands the compiled app from the `compile` job to the `sign` job of .github/workflows/release.yml.
//
//   node scripts/release/compiled.mjs pack <platform> <out folder>
//       In the compile job, after `tauri build --no-bundle`: copies the one file `tauri bundle`
//       needs (the app's executable) into <out folder>, with compiled.json recording its size and
//       checksum. The folder is uploaded as the workflow artifact `compiled-<platform>`.
//
//   node scripts/release/compiled.mjs place <platform> <downloaded folder>
//       In the sign job, on a fresh runner and a fresh checkout: puts that executable where
//       `tauri bundle` looks for it (target/<target>/release/), and nothing else. The artifact was
//       made by a job that ran every dependency's build code, so it is checked before it is used:
//       exactly the expected two files, plain files (no links), the size and checksum compiled.json
//       states, and nothing already at the destination. The executable is only packed and signed,
//       never run. (A workflow artifact also loses the executable bit, which this puts back.)
//
// Why not hand over the whole target/ folder: anything more than the executable could carry
// scripts or settings that the sign job would then read while it holds the signing keys.
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { isMain, PLATFORMS, ROOT, run, sha256, stop } from './lib.mjs';

export const MANIFEST = 'compiled.json';

/** The executable Tauri builds for `platformId`, and where `tauri bundle` looks for it. */
export function executable(platformId, root = ROOT) {
  const p = PLATFORMS.find((x) => x.id === platformId);
  if (!p) stop(`Unknown platform "${platformId}"`);
  const name = p.id === 'windows' ? 'thoughts.exe' : 'thoughts';
  return { name, path: join(root, 'target', ...(p.target ? [p.target] : []), 'release', name) };
}

export function pack({ platform, root = ROOT, out }) {
  const exe = executable(platform, root);
  if (!existsSync(exe.path) || !lstatSync(exe.path).isFile()) stop(`The compiled app is missing: ${exe.path}`);
  mkdirSync(out, { recursive: true });
  if (readdirSync(out).length) stop(`${out} is not empty`);
  copyFileSync(exe.path, join(out, exe.name));
  const record = { platform, file: exe.name, bytes: lstatSync(exe.path).size, sha256: sha256(exe.path) };
  writeFileSync(join(out, MANIFEST), JSON.stringify(record, null, 2) + '\n');
  return record;
}

export function place({ platform, root = ROOT, from }) {
  const exe = executable(platform, root);
  if (!existsSync(from)) stop(`${from} does not exist: the compiled app was not handed over`);
  const found = readdirSync(from).sort();
  const expected = [exe.name, MANIFEST].sort();
  if (found.join('\n') !== expected.join('\n')) {
    stop(`The compiled app for ${platform} should be exactly ${expected.join(' and ')}; found ${found.length ? found.join(', ') : 'nothing'}`);
  }
  for (const name of found) {
    if (!lstatSync(join(from, name)).isFile()) stop(`${name} in the compiled app is not a plain file`);
  }
  let record;
  try {
    record = JSON.parse(readFileSync(join(from, MANIFEST), 'utf8'));
  } catch {
    stop(`${MANIFEST} cannot be read`);
  }
  const given = join(from, exe.name);
  if (record?.platform !== platform || record?.file !== exe.name) stop(`${MANIFEST} describes ${record?.file} for ${record?.platform}, not ${exe.name} for ${platform}`);
  if (record.bytes !== lstatSync(given).size || record.sha256 !== sha256(given)) stop(`${exe.name} is not the file the compile job recorded (size or checksum differ)`);
  if (existsSync(exe.path)) stop(`${exe.path} already exists: the sign job must start from a fresh checkout`);
  mkdirSync(dirname(exe.path), { recursive: true });
  copyFileSync(given, exe.path);
  chmodSync(exe.path, 0o755);
  return { ...record, path: exe.path };
}

if (isMain(import.meta.url)) {
  await run(async () => {
    const [what, platform, folder] = process.argv.slice(2);
    if (!['pack', 'place'].includes(what) || !platform || !folder) stop('usage: compiled.mjs pack|place <platform> <folder>');
    const r = what === 'pack' ? pack({ platform, out: resolve(folder) }) : place({ platform, from: resolve(folder) });
    console.log(`${what === 'pack' ? 'Handed over' : 'In place'}: ${r.file} for ${platform}, ${r.bytes} bytes, sha256 ${r.sha256}`);
  });
}
