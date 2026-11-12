#!/usr/bin/env node
// Used by .github/workflows/release.yml (job "ci-status"). A release only goes out from a commit
// whose tests passed — but the usual way to release is "push main, then push the tag", and the
// push to main has already run the whole CI workflow for that very commit. This script looks for
// that run so the release does not test the same commit a second time:
//
//   CI passed for this commit on a branch push  -> tests-needed=false
//   CI is still running for it                  -> wait for the verdict, then decide
//   no run, or it failed or was cancelled       -> tests-needed=true (the release runs them itself)
//
// Any doubt (an API error, a wait that takes too long) means tests-needed=true: the tests are
// never skipped on a guess.
import { GitHub, isMain, run, setOutput, sleep, Stop, summary } from './lib.mjs';

/** 'passed' | 'pending' | 'none' for the CI runs of one commit. */
export function verdict(runs) {
  if (runs.some((r) => r.status === 'completed' && r.conclusion === 'success')) return 'passed';
  if (runs.some((r) => r.status !== 'completed')) return 'pending';
  return 'none';
}

export async function decide({
  gh,
  sha,
  defaultBranch,
  workflow = 'ci.yml',
  waitMs = 20 * 60_000, // how long to wait for a run that is still going
  appearMs = 90_000, // how long to wait for the branch push's run to show up at all
  pollMs = 15_000,
  now = () => Date.now(),
  pause = sleep,
  log = console.log,
}) {
  const start = now();
  const runsOf = async () => {
    const { data } = await gh.request(
      'GET',
      `/repos/${gh.repo}/actions/workflows/${workflow}/runs?head_sha=${sha}&event=push&per_page=50`,
    );
    return data?.workflow_runs ?? [];
  };

  // When the tag and the branch are pushed together, the branch's CI run may not exist yet.
  let isBranchHead = false;
  if (defaultBranch) {
    const { status, data } = await gh.request('GET', `/repos/${gh.repo}/branches/${encodeURIComponent(defaultBranch)}`, { allow: [404] });
    isBranchHead = status === 200 && data?.commit?.sha === sha;
  }

  for (;;) {
    const runs = await runsOf();
    const state = verdict(runs);
    if (state === 'passed') {
      const ok = runs.find((r) => r.status === 'completed' && r.conclusion === 'success');
      return { needed: false, reason: `CI already passed for this commit (${ok.html_url})` };
    }
    if (state === 'pending') {
      if (now() - start > waitMs) return { needed: true, reason: 'CI for this commit is still running after a long wait' };
      log('CI is still running for this commit; waiting for its verdict…');
      await pause(pollMs);
      continue;
    }
    if (runs.length === 0 && isBranchHead && now() - start < appearMs) {
      log(`No CI run for this commit yet, but it is the head of ${defaultBranch}; waiting for it to appear…`);
      await pause(Math.min(pollMs, 5000));
      continue;
    }
    return {
      needed: true,
      reason: runs.length === 0 ? 'no CI run exists for this commit' : 'CI did not pass for this commit (failed or cancelled)',
    };
  }
}

if (isMain(import.meta.url)) {
  await run(async () => {
    let result;
    try {
      result = await decide({
        gh: new GitHub(),
        sha: process.env.GITHUB_SHA,
        defaultBranch: process.env.DEFAULT_BRANCH || 'main',
      });
    } catch (e) {
      if (!(e instanceof Stop)) throw e;
      console.log(`::warning::Could not read the CI status (${e.message}); running the tests here.`);
      result = { needed: true, reason: 'the CI status could not be read' };
    }
    setOutput('tests-needed', result.needed);
    const line = result.needed
      ? `Tests run in this workflow: ${result.reason}.`
      : `Tests are not run again: ${result.reason}.`;
    console.log(line);
    summary(`**Tests.** ${line}`);
  });
}
