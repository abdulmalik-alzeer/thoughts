# Releasing Thoughts

How a new version goes from this repository to people's computers. Read it top to bottom the
first time; after that the [checklist](#release-checklist) is enough.

**Where things stand today**

- macOS is the only platform that has been built and run. What people have today is an
  **Apple Silicon build made by hand and shared privately**. The release workflow is set up to
  produce a universal `.dmg` (Apple Silicon + Intel), but no release has been published from it
  and the Intel half has never been run. The README states the user-facing version of this in
  its "Where it runs today" section; change the two together.
- Windows and Linux builds are wired up in CI but **have never been tested on a real machine**.
  Treat the first Windows/Linux installers as experiments, and run the
  [first-run test plan](#first-run-test-plan) before giving one to anybody.
- See [Known unverified](#known-unverified) for the full list of things nobody has checked.
- Nothing is signed yet. No signing certificate, no notarisation, no updater key exists. The
  release workflow works without them and produces unsigned installers.
- The in-app updater is built in but switched off until an updater key is configured (below).
  Until then "Check for Updates…" (and the weekly check under Settings › Maintenance, off by
  default) asks GitHub for the number of the latest published release and, when it is newer,
  offers its download page. While the repository is private that request finds nothing, so the
  app says it cannot tell. The user guides (README, CUSTOMISING) say the same; change them
  together.

## How a release is built

Pushing a git tag that starts with `v` (for example `v0.0.9`) runs
`.github/workflows/release.yml`. It builds, on GitHub's machines, all three platforms at the
same time:

| Platform | What you get |
|---|---|
| macOS | A universal `.dmg` (Apple Silicon + Intel), and the app as a `.app.tar.gz` |
| Windows | An `.exe` installer (NSIS, installs for the current user, no admin prompt) |
| Linux | A `.deb` and an `.AppImage` |

When every platform has built **and** the tests have passed for that commit, the files are
attached to a **draft** GitHub Release. A draft is invisible to the public. Nothing ships until
you open the draft and press **Publish**. If anything fails, nothing is attached at all: there is
never a half-filled release.

The same workflow can be started by hand as a [rehearsal](#rehearsal): it builds every installer
exactly as a release would, and publishes nothing.

`.github/workflows/ci.yml` runs on every push to `main` and on every pull request: the daemon
tests, the UI build and a compile check of the desktop shell on macOS, Windows and Linux, the UI
type checks, and the tests of the release pipeline's own scripts.

## Pipeline notes

- **Jobs:** `preflight` → the three platform `build` jobs side by side → `assemble` → `publish`.
  Beside the builds, `ci-status` decides whether `tests` has to run.
  - `preflight` stops the run before any build minute is spent when the tag does not match the
    version in `Cargo.toml`, or when a release for that tag is already published.
  - Each `build` job compiles, bundles, signs and hands its installers over as a workflow
    artifact. It can read the repository and nothing else.
  - `assemble` checks that the set is complete and intact (every platform, every file with the
    size and checksum its build recorded, nothing extra) and writes the updater manifest
    `latest.json` once, from all platforms.
  - `publish` is the only job that can write to the repository. It creates the draft, or reuses
    the draft an earlier attempt left and replaces its files.
- **Tests are not run twice.** The push to `main` already ran CI for the commit you tag. The
  release looks for that run: passed → it is not repeated; still running → the release waits for
  its verdict while the installers build; failed, cancelled or missing → the release runs the
  whole CI workflow itself. Either way, nothing is attached unless the tests passed for exactly
  that commit.
- **A published release is never changed.** Re-running the workflow or one of its jobs, or
  moving a tag to another commit, ends with an error once the release for that tag is public.
  To ship a fix, release a new version. (Switch on "release immutability" as well, see
  [Repository settings](#repository-settings-worth-switching-on).)
- **How long it takes.** The build jobs keep a cache of the compiled Rust dependencies. A tag
  can read caches saved on `main` but not another tag's, so the caches are saved from `main`
  only: by a rehearsal run on `main`, and by a "warm-up" run that starts by itself when a push
  to `main` changes the Rust dependencies or the workflow, and every Monday and Thursday
  (Windows and Linux only; macOS builds quickly even without a cache and its minutes cost ten
  times as much). The timed runs exist because GitHub deletes a cache nobody used for 7 days
  and a new Rust version (every six weeks) makes the old one useless; when the cache is still
  good they only check that and stop. Expect about **8 minutes** from tag to draft with warm
  caches and about **17 minutes** cold (the Windows build alone takes 14 minutes without a
  cache). A dependency change that no run on `main` has built yet costs little: the build starts
  from the previous cache. These figures are estimates from the 0.0.8 build times; replace them
  with measured ones after the first releases. Before 0.0.9 a release took about 38 minutes.
- **What it costs.** The repository is private, so GitHub counts runner minutes, each job
  rounded up to a whole minute: a macOS minute counts as ten, a Windows minute as two. A warm
  release is roughly 70 counted minutes (50 of them the macOS build), a cold one roughly 100, a
  cold rehearsal roughly 100 and a warm one roughly 50 (0.0.8: about 125 for the release run,
  plus 25 for testing the same commit on `main` first). A timed warm-up that finds the cache
  good costs about 6; one that rebuilds after a new Rust version about 45. Together that is
  roughly 80 a month; delete the `schedule:` lines in `release.yml` to save them, at the price
  of a cold release now and then. The included allowance of a free account is 2,000 a month.
- **Secrets** go to two steps only, "Windows signing certificate" and "Bundle and sign". Both
  run after the compile step, the step in which the build code of every Rust and npm dependency
  runs; that step gets no secret and no GitHub token (`scripts/tauri-ci.mjs` strips them, and
  the checkout leaves no token on disk). This stops a dependency from reading a signing key out
  of the environment while it is being built. It does not stop a dependency that deliberately
  plants something on the build machine for the later signing step to run; only a separate
  signing machine would. Treat the updater key accordingly, and see the protected environment
  under [Repository settings](#repository-settings-worth-switching-on).
- **Pinned actions:** every third-party action is pinned to a full commit SHA with the version
  in a comment; Dependabot (`.github/dependabot.yml`) proposes updates. `tauri-apps/tauri-action`
  is no longer used: the workflow calls the Tauri CLI itself (`tauri build --no-bundle`, then
  `tauri bundle`), and `scripts/release/` does what the action did (collecting the files,
  `latest.json`, the upload). Those scripts have tests: `node --test scripts/release/release.test.mjs`.
- **Linux packages** for the build are installed by `scripts/ci/apt-install.sh`, with timeouts
  and retries. A dead package mirror now fails the job within about ten minutes instead of
  stalling it for an hour; re-run the job when that happens.
- **Clippy** runs but is advisory for now (it reports existing warnings).
- **Content-security policy:** `connect-src` and `img-src` keep `http://127.0.0.1:*` because the
  background service picks a random port; the token-protected service is the real boundary.
  `dangerousDisableAssetCspModification` is set for `style-src` only, because Tauri would otherwise
  add a nonce to styles in every HTML asset, which blocks inline styles inside the sandboxed add-on
  frames. Script hashing stays on: the add-on runtime relies on it.
- **Linux `.deb`** installs update through the package manager; only the AppImage self-updates.

## Rehearsal

A rehearsal builds every installer exactly as a release would — same commands, same signing
steps when secrets exist — and keeps them as workflow artifacts. It creates no release and no
tag; none of its jobs can write to the repository. Use it to find out whether a release will
build *before* a tag exists, and to try the installers.

```bash
gh workflow run release.yml --ref main          # every platform
gh workflow run release.yml --ref main -f platforms=windows   # one platform: macos, windows or linux
gh run watch                                    # follow it; or open the Actions tab
gh run download <run id> --dir rehearsal        # the installers, when it has finished
```

Or: Actions tab → Release → **Run workflow**. The run's summary page lists every file with its
size and checksum and says whether signing and the updater were on. The artifacts are kept for
three days.

A rehearsal run on `main` also saves the build caches, so the release that follows it is the
quick kind. It costs minutes like a release does (see above), most of them for macOS; when you
only changed something that concerns one platform, rehearse that platform.

## The version number

The version is written in two places and both must match:

- `Cargo.toml` at the repository root, under `[workspace.package]`. Both Rust crates inherit it,
  and Tauri reads it too, because `src-tauri/tauri.conf.json` deliberately has **no** `version`
  key. Do not add one back.
- `package.json` (and `package-lock.json`).

Change both with one command, then let Cargo update its lock file:

```bash
node scripts/set-version.mjs 0.0.3
cargo check -p thoughts
```

The release workflow refuses to build if the tag does not match the version in `Cargo.toml`.

When a new version starts, it notices a background service left running by the old version,
stops it and starts its own. Any chat that was mid-answer at that moment is cut off.

## Release checklist

1. Make sure `main` is green in CI.
2. Set the version: `node scripts/set-version.mjs X.Y.Z`, then `cargo check -p thoughts`.
   Then bring the starter kit's history up to date: `git fetch --tags`, then
   `node scripts/kit-history.mjs`. It lists every starter file each earlier release shipped, so a
   library made by an older version is not asked for trust again because this release reworded
   one of the kit's routines (`--check` only says whether the file is current).
3. Commit (`Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`,
   `crates/thoughtsd/src/kit_history.rs`) and push to `main`.
4. Recommended: [rehearse](#rehearsal) with `gh workflow run release.yml --ref main` and wait for
   it. If it fails, fix `main` and rehearse again; no tag has been spent.
5. Tag and push the tag:
   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
6. Wait for the **Release** workflow (Actions tab). The platforms build side by side. If a job
   fails, nothing has been attached anywhere: open the run and press **Re-run failed jobs** —
   the platforms that succeeded are not built again. (Their installers are kept for one day;
   after that use **Re-run all jobs**.) If the code needs a fix instead, see
   [When a release run fails](#when-a-release-run-fails).
7. Open the draft release on GitHub. Check that every expected file is attached: the `.dmg`,
   the `.app.tar.gz`, the `.exe`, the `.deb`, the `.AppImage` (and, if the updater is
   configured, a `.sig` next to each installer and `latest.json`). The run's summary page lists
   them with their checksums.
8. Download each installer and run the [first-run test plan](#first-run-test-plan) for its
   platform. Do not publish a platform that has not passed it.
9. Write the release notes, then press **Publish release**. From that moment the release is
   final: the workflow will not touch it again.

### When a release run fails

- **A job failed for a reason outside the code** (a network error, a package mirror, a
  notarisation time-out): **Re-run failed jobs**. Only the failed jobs and the ones after them
  run again.
- **The code needs a fix:** fix it on `main`. As long as the release was not published you may
  reuse the version: delete the tag (`git push origin :refs/tags/vX.Y.Z`, `git tag -d vX.Y.Z`),
  delete the draft release if one was created, and tag the fixed commit. Once published, use a
  new version number.
- **"Release vX.Y.Z is already published":** that is the pipeline refusing to replace files
  people may already have downloaded. Release a new version.
- **"Tag … does not match the version in Cargo.toml":** the tag was pushed before the version
  commit, or with a typo. Delete the tag and push the right one.

To build a macOS installer by hand instead (unsigned, for this Mac's processor only):

```bash
source ~/.cargo/env && npx tauri build --bundles dmg
```

## Signing

Unsigned apps work but frighten people: macOS says the app "cannot be opened", Windows shows a
blue SmartScreen warning. Signing removes those warnings. Each part below is independent and
optional; the workflow skips whatever has no secrets.

Secrets are added on GitHub under **Settings → Secrets and variables → Actions**. Never put a
certificate, password or private key in the repository.

### macOS: Developer ID and notarisation

You need a paid Apple Developer Program membership.

1. In Xcode or on developer.apple.com, create a **Developer ID Application** certificate.
2. In Keychain Access, export it (certificate and private key) as a `.p12` file with a password.
3. Turn the file into one line of text: `base64 -i certificate.p12 | tr -d '\n' | pbcopy`
4. Find the identity name: `security find-identity -v -p codesigning`. It looks like
   `Developer ID Application: Your Name (TEAMID1234)`.
5. For notarisation, create an **app-specific password** at account.apple.com (Sign-In and
   Security → App-Specific Passwords). Tauri hands it to Apple's `notarytool` during the build.

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | The base64 text from step 3 |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` password from step 2 |
| `APPLE_SIGNING_IDENTITY` | The identity name from step 4 |
| `APPLE_ID` | Your Apple account email |
| `APPLE_PASSWORD` | The app-specific password from step 5 (not your Apple account password) |
| `APPLE_TEAM_ID` | The ten-character team ID |

The first three sign the app. Adding the last three also notarises it. Signing without
notarising is not enough for a download from the internet to open without a warning.

To check a finished build: `spctl -a -vv /Applications/Thoughts.app` should say
`accepted, source=Notarized Developer ID`.

Note: the macOS window uses a private macOS API for its translucent look
(`macOSPrivateApi`). That is fine for Developer ID distribution but rules out the Mac App Store.

### Windows: options

Pick one. None has been tried yet for this project.

1. **Stay unsigned.** Free. Users see SmartScreen ("Windows protected your PC") and must click
   *More info → Run anyway*.
2. **A code-signing certificate as a `.pfx` file** (OV certificate). This is what the workflow
   supports today. Add two secrets:

   | Secret | Value |
   |---|---|
   | `WINDOWS_CERTIFICATE` | The `.pfx` file as base64 (PowerShell: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))`) |
   | `WINDOWS_CERTIFICATE_PASSWORD` | The `.pfx` password |

   Be aware that since 2023 most certificate authorities only issue keys on hardware tokens or
   cloud HSMs, so an exportable `.pfx` may not be available to you. SmartScreen warnings also
   fade only gradually with an OV certificate.
3. **Azure Trusted Signing** (Microsoft's cloud signing service) or another cloud HSM. Usually
   the cheapest route that actually clears SmartScreen. This is **not wired up**: it needs a
   `bundle.windows.signCommand` in `src-tauri/tauri.windows.conf.json` and its own secrets. See
   the Tauri "Windows code signing" guide when you get there.

### Linux

`.deb` and `.AppImage` files are not signed. Nothing to set up.

## The updater

The app can check a small file, `latest.json`, see that a newer version exists, download it,
verify its signature and install it. The signature uses a keypair that is separate from the
Apple and Windows certificates. Without it the updater stays off.

**The key was deliberately not generated for you.** Whoever holds the private key can ship
code to every installed copy, and if it is lost, existing installs can never be updated again.
Generate it yourself and keep a backup somewhere safe (a password manager).

1. Generate the keypair, **outside the repository**:
   ```bash
   npx tauri signer generate -w ~/.tauri/thoughts-updater.key
   ```
   Choose a password when asked. This writes the private key to
   `~/.tauri/thoughts-updater.key` and the public key to `~/.tauri/thoughts-updater.key.pub`.
2. On GitHub, add:

   | Kind | Name | Value |
   |---|---|---|
   | Secret | `TAURI_SIGNING_PRIVATE_KEY` | The whole content of `thoughts-updater.key` |
   | Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password you chose |
   | Variable | `UPDATER_PUBKEY` | The whole content of `thoughts-updater.key.pub` |

   The public key is not secret, which is why it is a *variable*, not a secret.
3. That is all. On the next tag the workflow:
   - writes the public key and the endpoint into the build (through
     `scripts/release-config.mjs`, as the standard Tauri `plugins.updater` setting),
   - builds signed update files (a `.sig` next to each installer), writes one `latest.json` for
     all platforms, and attaches them to the release. A release in which an installer has no
     signature is refused.

   Run a [rehearsal](#rehearsal) first: this path has never been exercised, and a rehearsal
   shows the signatures and `latest.json` without publishing anything.

**Where the public key and endpoint live.** In the Tauri config under `plugins.updater`:

```json
{
  "bundle": { "createUpdaterArtifacts": true },
  "plugins": {
    "updater": {
      "pubkey": "<content of thoughts-updater.key.pub>",
      "endpoints": ["https://github.com/<owner>/<repo>/releases/latest/download/latest.json"]
    }
  }
}
```

CI generates exactly this into `src-tauri/release.conf.json` (a build-time file, do not commit
it) so that local and unconfigured builds stay updater-free. The endpoint is this repository's
"latest release" on GitHub. If you would rather hard-code it, the same block can go into
`src-tauri/tauri.conf.json`; then every build, local ones included, needs the private key in
`TAURI_SIGNING_PRIVATE_KEY` to finish.

**How it behaves in the app.** If the build has no public key or no endpoint, the updater is
not loaded: "Check for Updates…" then asks GitHub for the latest release number and offers its
download page (a Linux package says its updates come from the package manager). If it has both,
the app checks `latest.json`, and installs only an update whose signature matches the public key.

Things to know:

- "Latest release" on GitHub means the latest *published*, non-prerelease release. Drafts are
  never offered to users. Publishing the draft is what rolls the update out. The addresses in
  `latest.json` (`…/releases/download/vX.Y.Z/<file>`) only exist once the draft is published.
- The updater downloads from this repository's releases, without signing in. **While the
  repository is private nobody else can download them**, so the updater cannot work for other
  people until the repository (or at least its releases) is public.
- The first version that contains the public key cannot be reached by the updater from older
  versions. People install that one by hand; from then on updates are automatic.
- On Linux only the AppImage can update itself. `.deb` users reinstall.
- An unsigned macOS app can be updated, but macOS will complain again about each new copy.
  Sort out Apple signing before relying on the updater for Mac users.

## What a release must not break

A short list of behaviours the app's safety rests on. The details are in
[DEVELOPMENT.md](DEVELOPMENT.md#trust-model); the test plan below exercises each one.

- **Deny rules on every run.** Chat, Wrap, workflow, Reorganize and customise runs all get the
  generated deny rules for `.claude/`, `.mcp.json`, `.git/`, the app's data folder and the user's
  private folders.
- **Trust check.** A library the app did not create is checked for anything that can run a
  program, and its `.claude/**/*.md` files are compared byte-for-byte with the embedded starter.
  `starter/.claude/settings.json` is compared as exact JSON. A release that changes any starter
  skill or that settings file changes the baseline: libraries created by older versions will ask
  for trust once after updating. Say so in the release notes.
- **Plugin scope enforced by the daemon.** Private folders and the per-plugin write folders are
  checked in the daemon for every `scope=plugin` call, not in the UI.
- **Per-plugin manifests** (`plugins/<name>.json`, `views/<name>.json`) and the `apiVersion`
  check: a plugin or pack for a newer API is listed as unsupported, not loaded.
- **Apply signature.** Keep applies exactly the staged content the user reviewed, or fails with
  `CHANGED::`.
- **Library history.** Undo and restore create new history entries and never rewrite history;
  sync is refused while a Wrap is running or interrupted.
- **Line endings.** The plugin runtime is identified by a hash. `.gitattributes` keeps the
  hashed files LF on every platform; a Windows checkout with CRLF would produce a different hash.

## First-run test plan

Run this on a clean user account (no `~/.thoughts`, no previous install) for every platform in
the release, then once more as an upgrade over the previous version. Write down the platform,
OS version and result of each line in the release notes draft.

### Every platform

1. Install from the downloaded file exactly as a user would. Note every warning the system shows.
2. First launch: onboarding completes; a library is created from the starter kit in an empty
   folder. Check that `START-HERE.md`, `START-HERE.ar.md`, `presets/` and `presets/ar/` arrived.
3. Onboarding detects Claude Code and git, including the "not installed" and "not signed in"
   messages when they are missing.
4. Send one chat message; the answer streams. Setup (the init skill) runs once in English and
   once in Arabic on a second library: Arabic folder names are created and readable, and the
   welcome pages and the init skill are removed afterwards.
5. Press **Wrap**, review, save. Open **History**: the Wrap is listed. Undo it; restore one
   document to an earlier version.
6. With history turned off, History shows the "no history" message and nothing crashes.
7. Mark a folder private. Ask the assistant to read a file in it: refused. Open a customisation
   that reads the library: it cannot list that folder, including with different letter case.
8. Customise: request a small add-on. The review shows a summary and permissions; **Try it**
   changes nothing after closing; **Keep** applies; **Customise → History** undoes it. Check that
   the add-on's frame renders its styles (see Known unverified).
9. Export a feature pack, discard the add-on, import the pack: it arrives as a review and merges.
10. Create an automatic task for one minute ahead; it asks for approval, runs once, and appears
    as a chat.
11. Add a todo due today: it shows in the Calendar view and a notification appears. Export the
    calendar file and open it in the system calendar.
12. Sync to a test location: first upload, a change from a second copy, and a conflict (both
    resolutions). A backup branch exists afterwards.
13. Save a sync password with the default store, quit, reboot, sync again without re-entering
    it. Repeat with the "file" option and check the file's permissions.
14. Open a folder with a hook in `.claude/settings.json`, and one with an edited skill: both ask
    for trust and run nothing before the answer.
15. Close the window: the app stays in the tray or menu bar and a running chat continues. Reopen
    from the tray. **Quit Thoughts completely**: no process is left.
16. Log out and back in: the background service starts once (login item), not twice.
17. Switch the interface to Arabic: layout mirrors, native menus are in Arabic, error messages
    from the background service are in Arabic.
18. Damage `~/.thoughts/settings.json` by hand: the app restores from its backup and says so.
19. Updater, when configured: an older build is offered the new one, verifies it and restarts
    into it. When not configured: "updates are not configured in this build".
20. Uninstall following the README; confirm the app, `~/.thoughts` and the login item are gone
    and the library folder is untouched.

### macOS

- Apple Silicon and Intel separately (the universal build has two halves; test both).
- Gatekeeper on a downloaded copy: unsigned (right-click → Open) today; after signing,
  `spctl -a -vv` reports a notarised Developer ID.
- Keychain prompt on first password save, and no repeated prompts after an update.
- The LaunchAgent `app.thoughts.daemon.plist` is created, points at the installed app (not a
  translocated or `.dmg` path), and is replaced correctly on upgrade.
- The translucent window, and every item of the Recovery menu with a page that is blank or frozen
  (a customisation with `while(true){}`): undo, go back, discard, customisations off (still off
  after a restart), reload, open a fresh window, restart; each confirms and offers "Put it back".
- Quitting (⌘Q, closing the window with "keep running" off, the tray's "Quit Thoughts completely")
  while a chat runs and while a note has unsaved edits: the question names both; "Quit when
  finished" hides the window and quits once the work is done.
- Printing a document from the command palette prints the document, not a blank page.

### Windows

- SmartScreen on the installer; per-user install with no admin prompt; Start menu entry;
  uninstall from Settings → Apps.
- No console window flashes when the background service, git or the assistant starts.
- The assistant starts when Claude Code was installed through npm (the `claude.cmd` shim) and a
  prompt containing several lines and quotation marks arrives intact.
- A library path with spaces and non-ASCII characters (an Arabic user name) works; long paths
  and `\\?\` prefixes from canonical paths do not break private-folder or trust checks.
- Sync password is stored in Credential Manager and the git credential helper is found when the
  install path contains spaces.
- The plugin runtime hash matches (the file was checked out with LF; see `.gitattributes`).
- Autostart entry is created once and removed by the uninstaller, or documented as manual.
- Single instance: launching a second time focuses the first window.
- Tray icon, native menus (File · Edit · Window · Recovery · Help), and window state on a
  high-DPI and a second display.

### Linux

- `.deb` on a current Ubuntu LTS and the `.AppImage` on one other distribution.
- A desktop without a secret service (no keyring): the password falls back to the private file,
  with owner-only permissions, and the app says nothing misleading about a keychain. With a
  keyring: the password survives a reboot.
- AppImage: the autostart entry and the background service must not point at a temporary mount
  path under `/tmp`; check after moving the AppImage file and after a reboot.
- `.deb` installs are not offered an in-app update they cannot apply.
- Tray icon on GNOME (needs an extension) and KDE; behaviour when no tray exists (closing the
  window must not leave an unreachable background app).
- Native menu items that the platform does not support are absent, not dead.
- WebKitGTK rendering: fonts for Arabic, right-to-left layout, and the add-on frames.

## Known unverified

Things written or configured but never observed working. Each stays here until someone has
seen it work and noted the version and date.

- **Windows has never been run.** Compiles in CI; nothing else is known. Specific risks from the
  review: the `claude.cmd` shim and multi-line arguments, `\\?\` paths, the credential helper
  path with spaces, console windows, autostart.
- **Linux has never been run.** Compiles in CI. Specific risks: secrets lost on reboot with some
  keyring backends, AppImage paths under `/tmp` used for autostart and the background service,
  unsupported native menu items, `.deb` installs being offered updates.
- **The Intel half of the universal macOS build has never been run.**
- **Signing has never been exercised.** No Apple certificate, notarisation, Windows certificate
  or updater key exists. The signing and notarisation steps in the workflow, the updater's
  signature check, `latest.json` generation and an actual in-app update have never run.
- **Plugin frames under the bundled CSP.** In bundled builds Tauri adds a nonce to `<style>` in
  HTML assets, which may block inline styles inside add-on and view frames. This cannot be seen
  in `tauri dev`; it needs a look in an installed build (test plan step 8).
- **The release pipeline as rebuilt for 0.0.9.** It was written without being able to run it:
  its scripts are covered by tests and every action input was checked against the action's own
  definition, but parallel builds, the `tauri build --no-bundle` / `tauri bundle` split on
  every platform, the shared caches, the npm and crate Tauri versions agreeing, and the draft
  upload are only proven by a run. Note here the first rehearsal and the first tag that went
  through, with their durations.
- **Uninstall clean-up.** Settings → Maintenance → "Remove Thoughts from this computer" and the
  `scripts/uninstall-*.sh --purge` scripts remove the login item, the saved passwords and key
  (`thoughts-git`, `thoughts-ai`) and `~/.thoughts`; the Windows uninstaller removes the login item
  and, if ticked, the data folder, but not saved passwords. Only the macOS script has been run;
  the in-app removal on Windows and Linux and the Linux `secret-tool` step are untested.
- **Notifications for due dates** on Windows and Linux, and with macOS notification permission
  denied.
- **Standard native menu items** (Undo, Copy, Quit and so on) appearing in Arabic when the
  interface is Arabic.

## All secrets and variables in one place

| Name | Kind | Needed for |
|---|---|---|
| `APPLE_CERTIFICATE` | Secret | macOS signing |
| `APPLE_CERTIFICATE_PASSWORD` | Secret | macOS signing |
| `APPLE_SIGNING_IDENTITY` | Secret | macOS signing |
| `APPLE_ID` | Secret | macOS notarisation |
| `APPLE_PASSWORD` | Secret | macOS notarisation |
| `APPLE_TEAM_ID` | Secret | macOS notarisation |
| `WINDOWS_CERTIFICATE` | Secret | Windows signing (`.pfx` route) |
| `WINDOWS_CERTIFICATE_PASSWORD` | Secret | Windows signing (`.pfx` route) |
| `TAURI_SIGNING_PRIVATE_KEY` | Secret | Updater |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Secret | Updater |
| `UPDATER_PUBKEY` | Variable | Updater |

`GITHUB_TOKEN` is provided by GitHub automatically.

## What stays manual

- Bumping the version, tagging, writing release notes and pressing **Publish**.
- Trying each installer on a real machine before publishing, using the
  [first-run test plan](#first-run-test-plan).
- Windows and Linux in particular. The window settings, menus, tray icon, single-instance
  behaviour, the hidden-console flags for the background service, and the installers were
  written on a Mac and have not been run on those systems.
- Creating and renewing the Apple certificate (it expires after five years) and any Windows
  certificate, and keeping the updater private key backed up.
- Reviewing and merging the dependency updates Dependabot proposes, updating Tauri (both
  halves together), and acting when the Advisories workflow fails. See
  [Dependencies](#dependencies).
- The [repository settings](#repository-settings-worth-switching-on) below: nobody but the
  owner can switch them on.

## Dependencies

Every dependency version is pinned by a lock file (`Cargo.lock`, `package-lock.json`), and the
Actions by commit SHA. A fix released upstream therefore changes nothing until someone updates.
Two things keep that from going unnoticed:

- **Dependabot** (`.github/dependabot.yml`) opens pull requests: weekly for the Actions,
  monthly for the Rust crates and for the npm packages (one grouped pull request each for
  minor and patch releases; major releases come one by one). CI runs on each. They are not
  rebuilt every time `main` moves, to save CI minutes: comment `@dependabot rebase` on one to
  refresh it.
- **The Advisories workflow** (`.github/workflows/advisories.yml`) checks both lock files
  against the public vulnerability databases every Monday and whenever a lock file changes on
  `main`. It fails — and GitHub tells the owner by e-mail, unless those notifications were
  switched off — when an npm package that ships in the app has an advisory rated high or
  critical, or a Rust crate has a known vulnerability. Run it by
  hand with `gh workflow run advisories.yml`. The fix is nearly always to update the package.
  When an advisory truly does not apply to the app, accept it knowingly: for a Rust crate add
  `--ignore RUSTSEC-YYYY-NNNN` to the `audit` command in the workflow, with a comment saying
  why; npm has no such switch.

**Tauri is updated by hand.** The Tauri crates (`tauri`, `tauri-build`, `tauri-plugin-…`) and
the npm packages (`@tauri-apps/…`) must be on the same major.minor version, or `tauri build`
refuses to run — and CI does not run `tauri build`, so a one-sided update would first show up
as a failed release. Dependabot is told to leave Tauri's minor and major versions alone. To
update: bump both sides in one commit, then run a [rehearsal](#rehearsal).

## Repository settings worth switching on

These are settings on GitHub, not files in the repository; only the owner can change them.
None is required for the workflow to run.

- **Release immutability** (Settings → General → Releases). GitHub itself then refuses to
  change the files or the tag of a published release, whoever asks. The workflow already
  refuses; this closes the remaining door (a person, or another tool, replacing a file by
  hand). The draft-then-publish flow used here is the one it is designed for.
- **Dependabot alerts**, and **Dependabot security updates** (Settings → Advanced Security).
  Free. GitHub then reports vulnerable dependencies the day an advisory is published, and
  opens the pull request that fixes them without waiting for the monthly round.
- **Require actions to be pinned to a full-length commit SHA** (Settings → Actions → General).
  Makes the convention used in the workflows a rule.
- **A protected `release` environment for the signing secrets**, with a required reviewer:
  GitHub then asks a person to approve before a job may read the secrets. For a private
  repository, environment secrets need a paid plan (Pro or Team) and required reviewers need
  GitHub Enterprise; on a public repository both are free. Limiting the environment to tags
  `v*` (also possible on Pro or Team) already keeps the secrets away from every branch and
  rehearsal run. To adopt it: create the environment, move the signing secrets into it, and add
  `environment: release` to the `build` job in `release.yml`. With a tags-only rule a rehearsal
  on `main` would then be refused, so let only release runs name the environment. Worth doing
  before the updater key exists, because that key can ship code to every installed copy.
