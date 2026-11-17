#!/usr/bin/env node
// Used by .github/workflows/release.yml and scripts/release/local.mjs: runs the Tauri CLI for the
// two halves of a release build, and decides what each half may see in its environment.
//
//   node scripts/tauri-ci.mjs build --no-bundle …   COMPILE. This is where the build scripts and
//       procedural macros of every Rust dependency, and every npm package the UI build loads,
//       run. It gets no signing material and no GitHub token: whatever the workflow passed is
//       removed here before the CLI starts (and reported, because it should not have been passed).
//       It also loses the runner's "file command" variables (GITHUB_ENV, GITHUB_PATH, GITHUB_OUTPUT,
//       GITHUB_STATE, GITHUB_STEP_SUMMARY), the runner's own ACTIONS_* tokens and NODE_OPTIONS, so
//       dependency code cannot simply follow a variable to the file that sets the environment of
//       later steps. That is only a speed bump (the files are still on the runner's disk): what
//       actually keeps compile code away from the secrets is that in CI the signing happens in
//       another job, on a fresh runner (the `sign` job in release.yml).
//
//   node scripts/tauri-ci.mjs bundle …              PACKAGE AND SIGN. Runs no dependency build
//       code: it packs the compiled app with the system's tools and signs it. It receives the
//       signing secrets the workflow gave to that ONE step, after dropping the ones that are not
//       usable. Tauri treats a variable that is set but empty as "sign with this", and fails — so
//       a repository with no secrets (or only some) must not hand it empty values. It also loses
//       NODE_OPTIONS and the file-command variables, and is told to run no hook command
//       (`beforeBundleCommand`, `beforeBuildCommand`), so no npm script can run beside the keys.
//
//   macOS signing        needs APPLE_CERTIFICATE + APPLE_SIGNING_IDENTITY (else all APPLE_* dropped)
//   macOS notarisation   needs APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID (else those three dropped)
//   updater signing      needs TAURI_SIGNING_PRIVATE_KEY (else it and its password dropped)
//
// Nothing secret is printed or written anywhere.
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { isMain } from './release/lib.mjs';

const notarise = ['APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID'];
const apple = ['APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY', ...notarise, 'APPLE_API_KEY', 'APPLE_API_ISSUER', 'APPLE_API_KEY_PATH'];
const updaterKey = ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD'];
const windows = ['WINDOWS_CERTIFICATE', 'WINDOWS_CERTIFICATE_PASSWORD'];
const tokens = ['GITHUB_TOKEN', 'GH_TOKEN'];
/** Variables naming the files through which a step changes later steps (environment, PATH,
 *  outputs, state, the summary), and the runner's own tokens. Neither half of the build needs them. */
export const RUNNER_CHANNELS = [
  'GITHUB_ENV', 'GITHUB_PATH', 'GITHUB_OUTPUT', 'GITHUB_STATE', 'GITHUB_STEP_SUMMARY',
  'ACTIONS_RUNTIME_TOKEN', 'ACTIONS_RUNTIME_URL', 'ACTIONS_RESULTS_URL', 'ACTIONS_CACHE_URL',
  'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_URL',
];
/** Makes node load extra code into every node process started with it. */
export const NODE_INJECTION = ['NODE_OPTIONS'];
/** Given to `tauri bundle`: no hook command runs while the signing keys are in the environment. */
export const NO_HOOKS = JSON.stringify({ build: { beforeBundleCommand: null, beforeBuildCommand: null } });

/** The CLI arguments and environment for one half of the build. `log` lines are safe to print
 *  (variable names only, never a value). */
export function prepare(args, given, { platform = process.platform } = {}) {
  const env = { ...given };
  const log = [];
  const has = (k) => (env[k] || '').trim() !== '';
  const drop = (...keys) => keys.forEach((k) => delete env[k]);

  // The Tauri CLI never needs a GitHub token (the tools it downloads are public).
  drop(...tokens);
  const channels = [...RUNNER_CHANNELS, ...NODE_INJECTION].filter((k) => k in env);
  drop(...RUNNER_CHANNELS, ...NODE_INJECTION);

  const compileOnly = args[0] === 'build' && args.includes('--no-bundle');
  const cliArgs = [...args];
  if (compileOnly) {
    const present = [...apple, ...updaterKey, ...windows].filter(has);
    if (present.length) {
      log.push(`::warning::The compile step was given ${present.join(', ')}. Signing material belongs to the bundle step only; it was removed before compiling.`);
    }
    drop(...apple, ...updaterKey, ...windows);
    if (channels.length) log.push(`Compile: removed ${channels.join(', ')} from the environment`);
    log.push('Compile: no signing material and no GitHub token in the environment');
  } else {
    const sign = has('APPLE_CERTIFICATE') && has('APPLE_SIGNING_IDENTITY');
    if (!sign) drop('APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY', ...notarise);
    else if (!notarise.every(has)) drop(...notarise);
    const updater = has('TAURI_SIGNING_PRIVATE_KEY');
    if (!updater) drop(...updaterKey);
    drop(...windows); // the Windows certificate is used through the certificate store, by its thumbprint

    if (args[0] === 'bundle') cliArgs.push('--config', NO_HOOKS);
    if (platform === 'darwin') {
      log.push(`macOS signing: ${sign ? 'on' : 'off (unsigned build)'}; notarisation: ${sign && notarise.every(has) ? 'on' : 'off'}`);
      // On CI the bundler would skip arranging the disk image's window (icon positions). Keep the
      // arranged window, as every release so far has had it (tauri-action set the same default).
      if (args[0] === 'bundle') env.TAURI_BUNDLER_DMG_IGNORE_CI ??= 'true';
    }
    if (channels.length) log.push(`Bundle: removed ${channels.join(', ')} from the environment`);
    log.push(`Updater signing key: ${updater ? 'present' : 'absent'}`);
  }
  return { args: cliArgs, env, log };
}

if (isMain(import.meta.url)) {
  const { args, env, log } = prepare(process.argv.slice(2), process.env);
  for (const line of log) console.log(line);
  const cli = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
  const r = spawnSync(process.execPath, [cli, ...args], { stdio: 'inherit', env });
  process.exit(r.status ?? 1);
}
