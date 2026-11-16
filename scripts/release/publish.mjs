#!/usr/bin/env node
// Used by .github/workflows/release.yml. The only code in the pipeline that writes to a release,
// and it only ever writes to a DRAFT:
//
//   node scripts/release/publish.mjs --check-only
//       Preflight (read-only token): stop right away, before anything is built, when a release
//       for this tag has already been published.
//
//   node scripts/release/publish.mjs <folder with the installers> <folder from the assemble job>
//       Publish job (the one job with `contents: write`): checks every file against SHA256SUMS,
//       creates the draft release for the tag — or reuses the draft a previous attempt left —
//       and makes its files exactly the set this run built.
//
// A published release is never touched. Once people can download a version, its files do not
// change: a re-run of this workflow, a re-run of one job, or a tag that was moved to another
// commit all end here with an error instead of replacing installers under the same version
// number. To ship a fix, release a new version.
//
//   TAG, GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_SHA
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { changelogSection, RELEASE_NOTES } from './assemble.mjs';
import { GitHub, isMain, megabytes, ROOT, run, SAFE_NAME, sha256, sleep, Stop, stop, summary } from './lib.mjs';

const published = (tag) =>
  `Release ${tag} is already published, and a published release is never changed. ` +
  `To ship a fix, set a new version and push a new tag. (If ${tag} really must be rebuilt, a person has to delete the published release first.)`;

/** Preflight: is there a published release for this tag? (Drafts are invisible to a read-only token.) */
export async function refusePublished({ gh, tag }) {
  const { status, data } = await gh.request('GET', `/repos/${gh.repo}/releases/tags/${encodeURIComponent(tag)}`, { allow: [404] });
  if (status === 200 && data && data.draft === false) stop(published(tag));
}

/** The files to attach: release.json and SHA256SUMS must agree, and every file must match its hash. */
export function verifiedFiles({ assetsDir, metaDir, tag, commit }) {
  const release = JSON.parse(readFileSync(join(metaDir, 'release.json'), 'utf8'));
  if (release.schema !== 1 || release.mode !== 'release') stop('The assembled file list is not for a release');
  if (release.tag !== tag) stop(`The assembled files are for ${release.tag}, but this run is for ${tag}`);
  if (commit && release.commit && release.commit !== commit) stop(`The assembled files were built from ${release.commit}, but this run is for ${commit}`);

  const sums = new Map();
  for (const line of readFileSync(join(metaDir, 'SHA256SUMS'), 'utf8').split('\n').filter(Boolean)) {
    const m = line.match(/^([0-9a-f]{64}) {2}(.+)$/);
    if (!m || !SAFE_NAME.test(m[2]) || sums.has(m[2])) stop(`SHA256SUMS has a line that cannot be used: ${line}`);
    sums.set(m[2], m[1]);
  }
  if (!sums.size) stop('There is nothing to attach to the release');
  if (sums.size !== release.files.length || !release.files.every((f) => sums.get(f.name) === f.sha256)) {
    stop('SHA256SUMS and release.json do not list the same files');
  }
  return [...sums].map(([name, hash]) => {
    const path = join(name === 'latest.json' ? metaDir : assetsDir, name);
    if (!existsSync(path)) stop(`${name} is listed for the release but is missing`);
    if (sha256(path) !== hash) stop(`${name} does not match its checksum; refusing to upload it`);
    return { name, path, sha256: hash };
  });
}

/** `notes`: what the release page says (the changelog's entry for this version, as written). */
export async function publish({ gh, tag, commit, assetsDir, metaDir, notes = null, log = console.log, pauseMs = 3000 }) {
  const files = verifiedFiles({ assetsDir, metaDir, tag, commit });

  // Every release that carries this tag, drafts included (this token may see drafts).
  const ours = (await gh.list(`/repos/${gh.repo}/releases`)).filter((r) => r.tag_name === tag);
  if (ours.some((r) => !r.draft)) stop(published(tag));
  await refusePublished({ gh, tag }); // the same question asked the direct way
  if (ours.length > 1) {
    stop(`There are ${ours.length} draft releases for ${tag}. Delete the ones you do not want on the Releases page, then re-run this job.`);
  }

  let release = ours[0];
  if (release) {
    log(`Reusing the draft release for ${tag} (id ${release.id}); replacing its files.`);
    for (const asset of await gh.list(`/repos/${gh.repo}/releases/${release.id}/assets`)) {
      await gh.request('DELETE', `/repos/${gh.repo}/releases/assets/${asset.id}`, { allow: [404] });
    }
  } else {
    ({ data: release } = await gh.request('POST', `/repos/${gh.repo}/releases`, {
      body: { tag_name: tag, target_commitish: commit || undefined, name: `Thoughts ${tag}`, body: notes || RELEASE_NOTES, draft: true },
    }));
    if (!release?.id || release.draft !== true) stop(`Could not create a draft release for ${tag}`);
    log(`Created the draft release for ${tag} (id ${release.id}).`);
  }
  const assetsPath = `/repos/${gh.repo}/releases/${release.id}/assets`;

  for (const file of files) {
    const body = readFileSync(file.path);
    let done = false;
    let why = '';
    for (let attempt = 1; attempt <= 3 && !done; attempt++) {
      try {
        const { status, data } = await gh.request('POST', `${assetsPath}?name=${encodeURIComponent(file.name)}`, {
          base: gh.uploads,
          body,
          headers: { 'Content-Type': 'application/octet-stream' },
          allow: [422],
          timeoutMs: 15 * 60_000,
          attempts: 1,
        });
        done = status === 201 && data?.state === 'uploaded' && data?.size === body.length;
        why = done ? '' : `status ${status}, state ${data?.state ?? 'unknown'}`;
      } catch (e) {
        if (!(e instanceof Stop)) throw e;
        why = e.message;
      }
      if (!done) {
        // A failed upload can leave a broken file of that name behind; clear it before retrying.
        log(`Upload of ${file.name} failed (${why}); attempt ${attempt} of 3.`);
        await sleep(pauseMs * attempt);
        for (const asset of await gh.list(assetsPath)) {
          if (asset.name === file.name) await gh.request('DELETE', `/repos/${gh.repo}/releases/assets/${asset.id}`, { allow: [404] });
        }
      }
    }
    if (!done) stop(`Could not upload ${file.name}: ${why}`);
    log(`Uploaded ${file.name} (${megabytes(body.length)}).`);
  }

  // The draft must now hold exactly this run's files, and must still be a draft.
  const attached = await gh.list(assetsPath);
  const want = files.map((f) => f.name).sort().join('\n');
  const have = attached.filter((a) => a.state === 'uploaded').map((a) => a.name).sort().join('\n');
  if (want !== have) stop(`The draft release does not hold the expected files.\nExpected:\n${want}\nFound:\n${have}`);
  const { data: after } = await gh.request('GET', `/repos/${gh.repo}/releases/${release.id}`);
  if (after.draft !== true) stop(`Release ${tag} was published while its files were being uploaded. Check its files by hand.`);
  return { id: release.id, url: after.html_url, files };
}

if (isMain(import.meta.url)) {
  await run(async () => {
    const tag = process.env.TAG;
    if (!tag) stop('TAG is not set');
    const gh = new GitHub();
    if (process.argv[2] === '--check-only') {
      await refusePublished({ gh, tag });
      console.log(`No published release exists for ${tag}.`);
      return;
    }
    const [assetsDir, metaDir] = process.argv.slice(2);
    if (!assetsDir || !metaDir) stop('usage: publish.mjs <folder with the installers> <folder from the assemble job>');
    if (!gh.token) stop('GITHUB_TOKEN is not set');
    const changelog = join(ROOT, 'docs', 'CHANGELOG.md');
    const notes = existsSync(changelog) ? changelogSection(tag.replace(/^v/, ''), readFileSync(changelog, 'utf8')) : null;
    const result = await publish({ gh, tag, commit: process.env.GITHUB_SHA || '', assetsDir: resolve(assetsDir), metaDir: resolve(metaDir), notes });
    const text = [
      `## Draft release ${tag} is ready`,
      '',
      `${result.files.length} files attached. **Nothing is public yet**: open the draft, try the installers, write the notes, then press *Publish release*.`,
      '',
      result.url,
    ].join('\n');
    console.log(text);
    summary(text);
  });
}
