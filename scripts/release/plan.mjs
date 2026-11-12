#!/usr/bin/env node
// Preflight for .github/workflows/release.yml: decides what kind of run this is and which
// platforms it builds. The same workflow serves three purposes:
//
//   release    a tag `vX.Y.Z` was pushed       -> every installer, then a DRAFT release
//   rehearsal  started by hand (Run workflow)  -> every installer as workflow artifacts; no
//                                                 release, no tag, no job with write access
//   warm       a push to main changed the Rust dependencies or this pipeline, or the twice-weekly
//              schedule                        -> compile only, to refresh the build caches that
//                                                 tag builds read (Windows and Linux; a cold
//                                                 macOS build is already quick and costs 10x).
//                                                 Nothing to compile when the cache is current.
//
// Reads the trigger from the standard GITHUB_* variables and PLATFORMS (the rehearsal's
// "which installers" choice). Writes mode, version, tag, matrix, platforms and artifact-days
// to GITHUB_OUTPUT.
import { isMain, PLATFORMS, readVersion, run, setOutput, stop } from './lib.mjs';

export function plan({ eventName, refType, refName, choice, version }) {
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) stop(`Cargo.toml version "${version}" is not a version number`);

  let mode;
  if (eventName === 'workflow_dispatch') mode = 'rehearsal';
  else if (eventName === 'push' && refType === 'tag') mode = 'release';
  else if ((eventName === 'push' || eventName === 'schedule') && refType === 'branch') mode = 'warm';
  else stop(`The release workflow does not know what to do for a "${eventName}" event on a ${refType || 'unknown ref'}`);

  const tag = `v${version}`;
  if (mode === 'release' && refName !== tag) {
    stop(`Tag ${refName} does not match the version in Cargo.toml (${version}). Set the version with scripts/set-version.mjs, or push the tag ${tag}.`);
  }

  let ids;
  if (mode === 'release') {
    ids = PLATFORMS.map((p) => p.id); // a release is always complete
  } else if (mode === 'warm') {
    ids = ['windows', 'linux'];
  } else {
    const wanted = (choice || 'all').trim().toLowerCase();
    ids = wanted === 'all' ? PLATFORMS.map((p) => p.id) : [wanted];
    if (!ids.every((id) => PLATFORMS.some((p) => p.id === id))) {
      stop(`Unknown platform "${choice}". Use all, ${PLATFORMS.map((p) => p.id).join(', ')}.`);
    }
  }

  return {
    mode,
    version,
    tag,
    platforms: ids.join(','),
    matrix: JSON.stringify(PLATFORMS.filter((p) => ids.includes(p.id))),
    // Workflow artifacts are a hand-off between jobs. A release keeps its files in the draft
    // release, so one day is enough to re-run a failed job; a rehearsal's files are its result.
    'artifact-days': mode === 'release' ? 1 : 3,
  };
}

if (isMain(import.meta.url)) {
  await run(async () => {
    const result = plan({
      eventName: process.env.GITHUB_EVENT_NAME,
      refType: process.env.GITHUB_REF_TYPE,
      refName: process.env.GITHUB_REF_NAME,
      choice: process.env.PLATFORMS,
      version: readVersion(),
    });
    for (const [name, value] of Object.entries(result)) setOutput(name, value);
  });
}
