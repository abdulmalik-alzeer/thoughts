#!/usr/bin/env node
// Write down every file the starter kit shipped in each release, for the trust check.
//
//   node scripts/kit-history.mjs           rewrite crates/thoughtsd/src/kit_history.rs
//   node scripts/kit-history.mjs --check   only compare; exit 1 when that file is out of date
//   … --before X.Y.Z                       only the releases older than X.Y.Z (the release being
//                                          made: its own tag, once pushed, is not in its list)
//
// A library made by an earlier release keeps that release's copies of the kit's routines. The
// trust check (`trust.rs`) treats a library file that is byte for byte one of the versions listed
// here as the app's own, so a release that rewords a routine does not make every upgraded library
// ask for trust once although nobody changed anything in it.
//
// Where the list comes from:
//   - every release up to and including FROZEN (0.7.0): the committed kit_history.rs itself. Their
//     tags were deleted when the repository was made public and its history rewritten, so those
//     entries can no longer be read from git; the file is their only record and must never lose
//     one (libraries made by those releases rely on them). The script reads them back from the
//     file, checks them against FROZEN_SHA256, and refuses to go on if they were changed. A tag
//     of one of those releases that still exists somewhere is ignored: the file wins.
//   - every release after FROZEN: git, the `starter/` folder at its tag (`vX.Y.Z`), never the
//     working tree. Run it when preparing a release, after `git fetch --tags`, and commit the
//     result (see docs/RELEASING.md).
// Running it twice gives the same file, and with no tag after FROZEN it gives the committed file.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from './release/lib.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
export const OUT = 'crates/thoughtsd/src/kit_history.rs';

/** The last release whose entries are kept in the file only. */
export const FROZEN = '0.7.0';
/** The SHA-256 of those entries (`frozenFingerprint`), as they were when the old tags still existed. */
export const FROZEN_SHA256 = '1de49599f4a634f69d48fa17ba31fbcec42865c94b0120fdb34abc088895ea5e';

const nums = (v) => String(v).replace(/^v/, '').split('.').map(Number);
/** Negative when version `a` is older than `b` (`0.0.9` < `0.0.10`; a leading "v" is ignored). */
export const older = (a, b) => { const [x, y] = [nums(a), nums(b)]; for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i]; return 0; };

/** What `--before` says: null when it is not given, '' when it is not a version. */
export function parseBefore(argv) {
  const at = argv.indexOf('--before');
  // (A pre-release such as 0.1.0-rc.1 counts as its version: 0.1.0's own tag is left out too.)
  return at > 0 ? (/^(\d+\.\d+\.\d+)(-[0-9A-Za-z.-]+)?$/.exec(argv[at + 1] ?? '')?.[1] ?? '') : null;
}

/** The releases and rows of a kit_history.rs. Throws when the file is not one this script wrote. */
export function parseHistory(text) {
  const releasesLine = /^pub const RELEASES: &\[&str\] = &\[(.*)\];$/m.exec(text);
  if (!releasesLine) throw new Error(`${OUT} has no RELEASES line`);
  const list = (inner) => (inner.trim() ? JSON.parse(`[${inner}]`) : []);
  const releases = list(releasesLine[1]);
  const body = /^pub const SHIPPED: &\[\(&str, &str, &\[&str\]\)\] = &\[\n([\s\S]*?)^\];$/m.exec(text);
  if (!body) throw new Error(`${OUT} has no SHIPPED list`);
  const rows = [];
  for (const line of body[1].split('\n').filter(Boolean)) {
    const m = /^ {4}\(("(?:[^"\\]|\\.)*"), "([0-9a-f]{64})", &\[(.*)\]\),$/.exec(line);
    if (!m) throw new Error(`${OUT}: a line of SHIPPED could not be read: ${line.slice(0, 80)}`);
    rows.push({ rel: JSON.parse(m[1]), sha: m[2], releases: list(m[3]) });
  }
  return { releases, rows };
}

/** The entries for FROZEN and the releases before it, read from a kit_history.rs. */
export function frozenPart(text) {
  const { releases, rows } = parseHistory(text);
  const keep = (v) => older(v, FROZEN) <= 0;
  return {
    releases: releases.filter(keep),
    rows: rows.map((r) => ({ ...r, releases: r.releases.filter(keep) })).filter((r) => r.releases.length),
  };
}

export const frozenFingerprint = ({ releases, rows }) => createHash('sha256').update(JSON.stringify({ releases, rows })).digest('hex');

/** The text of kit_history.rs: the frozen entries from `current` (the committed file) plus every
 *  release tagged after FROZEN (and before `before`, when given), read with `git`. */
export function kitHistory({ current, before = null, git, fingerprint = FROZEN_SHA256 }) {
  if (before !== null && older(before, FROZEN) <= 0) throw new Error(`--before ${before}: the releases up to ${FROZEN} are kept in ${OUT} as they are and cannot be listed again.`);
  const frozen = frozenPart(current);
  if (!frozen.releases.includes(FROZEN) || frozenFingerprint(frozen) !== fingerprint) {
    throw new Error(`The entries for ${FROZEN} and earlier in ${OUT} are not the ones recorded (FROZEN_SHA256 in scripts/kit-history.mjs). They cannot be read from git any more: restore them from the file's git history (git log -p -- ${OUT}).`);
  }
  const tags = git(['tag', '--list', 'v*']).toString().split('\n').map((t) => t.trim()).filter((t) => /^v\d+\.\d+\.\d+$/.test(t))
    .filter((t) => older(t, FROZEN) > 0 && (before === null || older(t, before) < 0)).sort(older);

  // (path inside the kit, sha256) → the releases that shipped exactly those bytes there.
  const shipped = new Map(frozen.rows.map((r) => [`${r.rel}\0${r.sha}`, { ...r, releases: [...r.releases] }]));
  const blobHash = new Map();
  for (const tag of tags) {
    // NUL-separated, so no path is ever quoted: `<mode> <type> <object>\t<path>`.
    for (const rec of git(['ls-tree', '-r', '-z', tag, '--', 'starter']).toString().split('\0')) {
      if (!rec) continue;
      const [meta, path] = [rec.slice(0, rec.indexOf('\t')), rec.slice(rec.indexOf('\t') + 1)];
      const [mode, type, object] = meta.split(' ');
      // Plain files only: the kit has never shipped a link, and a link is never the app's own file.
      if (type !== 'blob' || !['100644', '100755'].includes(mode)) continue;
      const rel = path.slice('starter/'.length);
      if (!rel || /[\u0000-\u001f"\\]/.test(rel)) throw new Error(`unexpected path in ${tag}: ${JSON.stringify(path)}`);
      if (!blobHash.has(object)) blobHash.set(object, createHash('sha256').update(git(['cat-file', 'blob', object])).digest('hex'));
      const key = `${rel}\0${blobHash.get(object)}`;
      if (!shipped.has(key)) shipped.set(key, { rel, sha: blobHash.get(object), releases: [] });
      shipped.get(key).releases.push(tag.slice(1));
    }
  }

  const releases = [...frozen.releases, ...tags.map((t) => t.slice(1))];
  const rows = [...shipped.values()].sort((a, b) => (a.rel < b.rel ? -1 : a.rel > b.rel ? 1 : older(a.releases[0], b.releases[0])));
  const text = `//! Generated by \`node scripts/kit-history.mjs\` from the release tags. Do not edit by hand.
//!
//! Every file the starter kit has shipped in a release: its path inside the kit, the SHA-256 of
//! its bytes, and the releases that shipped exactly those bytes there. \`trust.rs\` reads it.

/// The releases this list was made from, oldest first.
pub const RELEASES: &[&str] = &[${releases.map((v) => JSON.stringify(v)).join(', ')}];

/// (path inside the kit, sha256 of the file, releases that shipped it).
pub const SHIPPED: &[(&str, &str, &[&str])] = &[
${rows.map((r) => `    (${JSON.stringify(r.rel)}, "${r.sha}", &[${r.releases.map((v) => JSON.stringify(v)).join(', ')}]),`).join('\n')}
];
`;
  return { text, releases, rows, added: tags };
}

if (isMain(import.meta.url)) {
  const check = process.argv.includes('--check');
  const before = parseBefore(process.argv);
  if (before === '') {
    console.error('--before takes a version number such as 0.7.1.');
    process.exit(2);
  }
  const out = join(ROOT, OUT);
  let current = '';
  try { current = readFileSync(out, 'utf8'); } catch { /* reported below */ }
  if (!current) {
    console.error(`${OUT} is missing. It holds the only record of the releases up to ${FROZEN}: restore it from git.`);
    process.exit(1);
  }
  let made;
  try {
    made = kitHistory({ current, before, git: (args) => execFileSync('git', args, { cwd: ROOT, maxBuffer: 256 << 20 }) });
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
  const counts = `${made.releases.length} releases (${made.added.length} read from tags after ${FROZEN}), ${made.rows.length} distinct files`;
  if (check) {
    if (current !== made.text) {
      console.error(`${OUT} is out of date. Run: node scripts/kit-history.mjs`);
      process.exit(1);
    }
    console.log(`kit_history.rs is up to date: ${counts}.`);
  } else {
    writeFileSync(out, made.text);
    console.log(`Wrote kit_history.rs: ${counts}.`);
  }
}
