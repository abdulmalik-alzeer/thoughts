#!/usr/bin/env node
// Used by .github/workflows/release.yml (job "assemble"), once, after every platform job has
// finished. It has the installers of all platforms in one folder (the downloaded workflow
// artifacts) and:
//
//   1. checks the set is complete and intact: every expected platform reported, every file is
//      there with the size and sha256 its build job recorded, nothing unexpected is present,
//      all platforms were built from this commit and for this version;
//   2. writes latest.json, the updater manifest, when the updater is configured — in one go,
//      from all platforms (the platform jobs used to merge their entries into the release one
//      after another, which is why they could not run in parallel);
//   3. writes SHA256SUMS and release.json: the exact list of files the publish job may attach.
//
// This job has no write access to anything. The publish job re-checks every file against
// SHA256SUMS before it uploads.
//
//   node scripts/release/assemble.mjs <folder with the installers> <output folder>
//   MODE, VERSION, TAG, PLATFORMS (comma separated), GITHUB_REPOSITORY, GITHUB_SHA
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { isMain, megabytes, PLATFORMS, ROOT, run, SAFE_NAME, sha256, stop, summary } from './lib.mjs';

const UPDATER_OS = { macos: 'darwin', windows: 'windows', linux: 'linux' };
// What an installed copy is updated with when it does not say how it was installed. A .deb is
// only ever offered to a copy that was installed from a .deb.
const DEFAULT_FOR_PLATFORM = { app: true, nsis: true, appimage: true, deb: false };
/** Said when the changelog has no entry for the version being released. */
export const RELEASE_NOTES = 'See the assets below to download and install this version.';

/** The entry for `version` in a changelog written the way docs/CHANGELOG.md is: a `## <version>`
 *  heading, then the entry, up to the next `## `. Null when there is none. */
export function changelogSection(version, changelog) {
  const lines = String(changelog ?? '').split(/\r?\n/);
  const start = lines.findIndex((l) => l.trim() === `## ${version}`);
  if (start < 0) return null;
  let end = lines.findIndex((l, i) => i > start && /^## /.test(l));
  if (end < 0) end = lines.length;
  return lines.slice(start + 1, end).join('\n').trim() || null;
}

/** The entry as plain text, for the `notes` of latest.json. Copies older than 0.1.0 show those
 *  notes in their update dialog, as they are, so heading and emphasis marks go, bullets become
 *  "•", links keep their words, and it is cut where a dialog stops being readable. (From 0.1.0 the
 *  dialog shows only the app's own words and a link to the release page; each version's own
 *  change log is in Help › What's new.) Null when the changelog has no entry for `version`. */
export function notesFor(version, changelog, { max = 1200 } = {}) {
  const section = changelogSection(version, changelog);
  if (!section) return null;
  const out = [];
  for (const raw of section.split('\n')) {
    let line = raw.trimEnd();
    if (/^#{3,}\s+/.test(line)) line = line.replace(/^#{3,}\s+/, '');
    else if (/^\s*[-*]\s+/.test(line)) line = `• ${line.replace(/^\s*[-*]\s+/, '')}`;
    line = line.replace(/\*\*(.+?)\*\*/g, '$1').replace(/`([^`]*)`/g, '$1').replace(/\[([^\]]+)\]\([^)]*\)/g, '$1');
    if (!line.trim() && !(out.at(-1) ?? '').trim()) continue; // never two empty lines in a row
    out.push(line);
  }
  let text = out.join('\n').trim();
  if (text.length > max) {
    const cut = text.lastIndexOf('\n', max);
    text = `${text.slice(0, cut > 0 ? cut : max).trimEnd()}\n…`;
  }
  return text || null;
}

/** The updater manifest (Tauri's static JSON format) for these builds. `notes` is what the
 *  update dialog of a copy older than 0.1.0 shows; `notesAr` the same for such a copy set to
 *  Arabic (an extra field the updater library ignores). Copies from 0.1.0 on never show either:
 *  the manifest is not signed. Both are still written, for the older copies. */
export function updaterManifest({ infos, version, tag, repo, signatureOf, server = 'https://github.com', date = new Date(), notes = null, notesAr = null }) {
  const platforms = {};
  for (const info of infos) {
    const os = UPDATER_OS[info.platform];
    for (const file of info.files) {
      if (!file.signature) continue;
      if (!(file.kind in DEFAULT_FOR_PLATFORM)) stop(`${file.name}: no updater entry is defined for a "${file.kind}" file`);
      if (!file.arch) stop(`${file.name}: the build did not say which processor it is for`);
      // A universal macOS build serves both processors.
      const archs = file.arch === 'universal' ? ['aarch64', 'x86_64', 'universal'] : [file.arch];
      const entry = {
        signature: signatureOf(file.signature),
        // Where the file lives once the draft is published. A draft is never offered to anyone.
        url: `${server}/${repo}/releases/download/${tag}/${file.name}`,
      };
      for (const arch of archs) {
        if (DEFAULT_FOR_PLATFORM[file.kind]) platforms[`${os}-${arch}`] = entry;
        platforms[`${os}-${arch}-${file.kind}`] = entry;
      }
    }
  }
  return { version, notes: notes || RELEASE_NOTES, ...(notesAr ? { notes_ar: notesAr } : {}), pub_date: date.toISOString(), platforms };
}

/** `partial`: a release of fewer than all platforms, which only the local release command asks
 *  for (scripts/release/local.mjs builds macOS alone). The pipeline never does. */
export function assemble({ assetsDir, metaDir, mode, version, tag, platforms, repo, commit, date, server, partial = false, notes = null, notesAr = null }) {
  if (!['release', 'rehearsal'].includes(mode)) stop(`assemble does not run in "${mode}" mode`);
  if (!existsSync(assetsDir)) stop(`${assetsDir} does not exist: no installers were downloaded`);
  const expected = [...platforms].sort();
  const all = PLATFORMS.map((p) => p.id).sort();
  if (!expected.length || !expected.every((id) => all.includes(id))) stop(`Unknown platform list "${platforms.join(',')}"`);
  if (mode === 'release' && !partial && expected.join() !== all.join()) {
    stop(`A release needs every platform (${all.join(', ')}), but this run built ${expected.join(', ')}`);
  }

  const present = readdirSync(assetsDir, { withFileTypes: true });
  const folder = present.find((e) => !e.isFile());
  if (folder) stop(`Unexpected folder "${folder.name}" among the installers`);
  const names = new Set(present.map((e) => e.name));

  // 1. One report per expected platform, and no others.
  const reported = [...names].filter((n) => /^build-info-.*\.json$/.test(n)).map((n) => n.slice('build-info-'.length, -'.json'.length)).sort();
  for (const id of expected) if (!reported.includes(id)) stop(`The ${id} build did not deliver its installers (build-info-${id}.json is missing)`);
  for (const id of reported) if (!expected.includes(id)) stop(`Installers from an unexpected platform "${id}" are present`);

  const infos = expected.map((id) => JSON.parse(readFileSync(join(assetsDir, `build-info-${id}.json`), 'utf8')));
  const files = [];
  for (const info of infos) {
    const where = `build-info-${info.platform}.json`;
    if (info.schema !== 1) stop(`${where}: unknown format`);
    if (info.version !== version) stop(`${where}: built version ${info.version}, expected ${version}`);
    if (commit && info.commit && info.commit !== commit) stop(`${where}: built from commit ${info.commit}, but this run is for ${commit}`);
    if (!Array.isArray(info.files) || !info.files.length) stop(`${where}: lists no files`);
    for (const f of info.files) {
      if (!SAFE_NAME.test(f.name)) stop(`${where}: "${f.name}" is not a safe release file name`);
      if (/^build-info-|^latest\.json$|^SHA256SUMS$|^release\.json$/.test(f.name)) stop(`${where}: "${f.name}" is a reserved name`);
      if (files.some((x) => x.name === f.name)) stop(`Two builds delivered a file named ${f.name}`);
      const path = join(assetsDir, f.name);
      if (!names.has(f.name)) stop(`${f.name} is listed by the ${info.platform} build but was not delivered`);
      if (statSync(path).size !== f.bytes) stop(`${f.name}: size differs from what the ${info.platform} build recorded`);
      if (sha256(path) !== f.sha256) stop(`${f.name}: content differs from what the ${info.platform} build recorded`);
      if (f.signature && !info.files.some((x) => x.name === f.signature && x.kind === 'signature')) stop(`${f.name}: its signature file ${f.signature} is not listed`);
      files.push({ ...f, platform: info.platform });
    }
  }
  // Nothing rides along that no build declared.
  for (const n of names) {
    if (!files.some((f) => f.name === n) && !/^build-info-.*\.json$/.test(n)) stop(`Unexpected file "${n}" among the installers`);
  }

  // 2. The updater manifest. Either every platform has signed update files or none has.
  const updater = infos[0].updater === true;
  if (infos.some((i) => (i.updater === true) !== updater)) {
    stop(`The updater is on for some platforms and off for others (${infos.map((i) => `${i.platform}: ${i.updater ? 'on' : 'off'}`).join(', ')})`);
  }
  mkdirSync(metaDir, { recursive: true });
  let manifest = null;
  if (updater) {
    manifest = updaterManifest({
      infos,
      version,
      tag,
      repo,
      date,
      server,
      notes,
      notesAr,
      signatureOf: (name) => readFileSync(join(assetsDir, name), 'utf8').trim(),
    });
    for (const info of infos) {
      const os = UPDATER_OS[info.platform];
      if (!Object.keys(manifest.platforms).some((k) => k.startsWith(`${os}-`))) stop(`The updater is on but the ${info.platform} build has no signed update file`);
    }
    const text = JSON.stringify(manifest, null, 2) + '\n';
    writeFileSync(join(metaDir, 'latest.json'), text);
    files.push({ name: 'latest.json', kind: 'manifest', bytes: Buffer.byteLength(text), sha256: sha256(join(metaDir, 'latest.json')), signature: null, platform: 'all' });
  }

  // 3. The list the publish job works from.
  files.sort((a, b) => (a.name < b.name ? -1 : 1));
  writeFileSync(join(metaDir, 'SHA256SUMS'), files.map((f) => `${f.sha256}  ${f.name}\n`).join(''));
  const signed = Object.assign({}, ...infos.map((i) => i.signed ?? {})); // each build reports its own platform only
  const release = { schema: 1, mode, version, tag, commit, platforms: expected, updater, signed, files };
  writeFileSync(join(metaDir, 'release.json'), JSON.stringify(release, null, 2) + '\n');
  return release;
}

function report(release) {
  const yes = (v) => (v ? 'yes' : 'no');
  const built = new Set(release.platforms);
  const lines = [
    release.mode === 'release'
      ? `## Installers for ${release.tag}`
      : `## Rehearsal of ${release.tag}: nothing was published`,
    '',
    release.mode === 'release'
      ? 'Every platform built. The files below go to the draft release.'
      : 'No release and no tag were created. The installers are in the **Artifacts** section of this run (`installers-…`).',
    '',
    '| File | Size | SHA-256 |',
    '|---|---|---|',
    ...release.files.map((f) => `| \`${f.name}\` | ${megabytes(f.bytes)} | \`${f.sha256}\` |`),
    '',
    `- In-app updater: **${release.updater ? 'on' : 'off'}** (${release.updater ? 'latest.json written' : 'no updater key is configured, so no latest.json'})`,
  ];
  if (built.has('macos')) lines.push(`- macOS signed: **${yes(release.signed.macos)}**, notarised: **${yes(release.signed.notarized)}**`);
  if (built.has('windows')) lines.push(`- Windows signed: **${yes(release.signed.windows)}**`);
  return lines.join('\n');
}

if (isMain(import.meta.url)) {
  await run(async () => {
    const [assetsDir, metaDir] = process.argv.slice(2);
    if (!assetsDir || !metaDir) stop('usage: assemble.mjs <folder with the installers> <output folder>');
    const changelog = (file) => (existsSync(join(ROOT, file)) ? readFileSync(join(ROOT, file), 'utf8') : '');
    const release = assemble({
      assetsDir: resolve(assetsDir),
      metaDir: resolve(metaDir),
      mode: process.env.MODE,
      partial: process.env.PARTIAL === 'true',
      notes: notesFor(process.env.VERSION, changelog('docs/CHANGELOG.md')),
      notesAr: notesFor(process.env.VERSION, changelog('docs/ar/CHANGELOG.md')),
      version: process.env.VERSION,
      tag: process.env.TAG,
      platforms: (process.env.PLATFORMS || '').split(',').filter(Boolean),
      repo: process.env.GITHUB_REPOSITORY,
      commit: process.env.GITHUB_SHA || '',
      server: process.env.GITHUB_SERVER_URL || undefined,
    });
    const text = report(release);
    console.log(text);
    summary(text);
  });
}
