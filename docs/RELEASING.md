# Releasing Thoughts

How a new version goes from this repository to people's computers. Read it top to bottom the
first time; after that the [checklist](#release-checklist) is enough.

**Where things stand today**

- The repository is **public** (from 0.7.1, 2026-11-24): the code, the Releases page and the
  updater's `latest.json` can be read by anyone, with no sign-in. 0.7.1 is the first release of
  the public repository and its only tag: the history was rewritten and every earlier release
  and tag deleted. Copies of 0.0.12 to 0.7.0 still find it (see
  [Older copies and the public repository](#older-copies-and-the-public-repository)).
- macOS is the only platform that has been built and run. The releases are universal `.dmg`
  files (Apple Silicon + Intel) made on a Mac with the local release command; the Intel half has
  never been run. The README states the user-facing version of this in
  its "Where it runs today" section; change the two together.
- Windows and Linux builds are wired up in CI but **have never been tested on a real machine**.
  Treat the first Windows/Linux installers as experiments, and run the
  [first-run test plan](#first-run-test-plan) before giving one to anybody.
- See [Known unverified](#known-unverified) for the full list of things nobody has checked.
- No Apple or Windows certificate exists and nothing is notarised: installers are unsigned
  (decided 2026-11-15: no Apple developer account for now). The release workflow works without
  them.
- The in-app updater is in every build made with the updater key (see
  [The updater](#the-updater)), which is every release from 0.0.12 on: 0.0.11 and older cannot
  update themselves and are updated by hand once. It asks the public address only, with no
  sign-in. The GitHub fallback for the time the repository was private (`thoughts updates gh
  on`, in 0.0.12 to 0.7.0) was removed in 0.7.1. The user guides (README, CUSTOMISING, PRIVACY)
  describe all of this; change them together.
- Releases are made on a Mac with one command while the pipeline cannot run (see
  [A release made on this Mac](#a-release-made-on-this-mac)).
- An update was rehearsed on this Mac on 2026-11-16. [Known unverified](#known-unverified)
  says what that showed and what it did not.

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
tests, the UI build and the desktop shell's tests (`cargo test -p thoughts`, which also builds
it) on macOS, Windows and Linux, the UI type checks, and the tests of the release pipeline's own
scripts.

## Pipeline notes

- **Jobs:** `preflight` → the three platform `compile` jobs side by side → one `sign` job per
  platform → `assemble` → `publish`.
  Beside the builds, `ci-status` decides whether `tests` has to run.
  - `preflight` stops the run before any build minute is spent when the tag does not match the
    version in `Cargo.toml`, or when a release for that tag is already published.
  - Each `compile` job builds the app with no secret at all and hands over one file, the
    compiled executable (with its size and checksum), as a workflow artifact.
  - Each `sign` job starts on a fresh machine from a fresh checkout, installs the Tauri CLI with
    `npm ci --ignore-scripts`, checks the executable and puts it in place
    (`scripts/release/compiled.mjs`), then packs, signs and hands its installers over. Both
    kinds of job can read the repository and nothing else.
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
- **How long it takes.** The compile jobs keep a cache of the compiled Rust dependencies. A tag
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
- **What it costs.** On a public repository GitHub's standard runners cost nothing. The figures
  below are from the time the repository was private, when GitHub counted runner minutes, each
  job rounded up to a whole minute: a macOS minute counted as ten, a Windows minute as two. A warm
  release is roughly 70 counted minutes (50 of them the macOS build), a cold one roughly 100, a
  cold rehearsal roughly 100 and a warm one roughly 50 (0.0.8: about 125 for the release run,
  plus 25 for testing the same commit on `main` first). A timed warm-up that finds the cache
  good costs about 6; one that rebuilds after a new Rust version about 45. Together that is
  roughly 80 a month; delete the `schedule:` lines in `release.yml` to save them, at the price
  of a cold release now and then. The included allowance of a free account is 2,000 a month.
- **Secrets** go to two steps only, "Windows signing certificate" and "Bundle and sign", both in
  the `sign` job. The build code of every Rust and npm dependency runs in the `compile` job, on
  another machine, which gets no secret and no GitHub token (`scripts/tauri-ci.mjs` strips them,
  and the checkout leaves no token on disk). Before 0.1.0 both halves shared one job, so a
  dependency could plant something for the signing step to run (a line in `GITHUB_ENV` such as
  `NODE_OPTIONS=--import=…`, or a changed `node_modules` or `target/`). Now nothing from the
  compile machine reaches the signing but the executable, which is checked and only packed and
  signed, never run. The sign job runs no project dependency code while it holds the secrets:
  no build cache and no npm cache, `npm ci --ignore-scripts`, nothing compiled, `tauri bundle`
  told to run no hook command, and of the installed packages only the Tauri CLI itself is
  started (it is the tool that signs, at the version in the lock file). `tauri-ci.mjs` also
  removes `GITHUB_ENV`, `GITHUB_PATH`, `GITHUB_OUTPUT`, `GITHUB_STATE`, `GITHUB_STEP_SUMMARY`,
  the runner's `ACTIONS_*` tokens and `NODE_OPTIONS` from the compiler's environment, a speed
  bump on top. Still trusted: the Tauri CLI, GitHub's runner image and the system's packing and
  signing tools. Treat the updater key accordingly, and see the protected environment under
  [Repository settings](#repository-settings-worth-switching-on). **This split has not been run
  yet** (see [Known unverified](#known-unverified)).
- **Pinned actions:** every third-party action is pinned to a full commit SHA with the version
  in a comment; Dependabot (`.github/dependabot.yml`) proposes updates. `tauri-apps/tauri-action`
  is no longer used: the workflow calls the Tauri CLI itself (`tauri build --no-bundle`, then
  `tauri bundle`), and `scripts/release/` does what the action did (collecting the files,
  `latest.json`, the upload). Those scripts have tests: `node --test scripts/release/release.test.mjs`.
- **Linux packages** for the build are installed by `scripts/ci/apt-install.sh`, with timeouts
  and retries. A dead package mirror now fails the job within about ten minutes instead of
  stalling it for an hour; re-run the job when that happens.
- **Clippy** runs but is advisory for now (it reports existing warnings).
- **Content-security policy:** `connect-src` keeps `http://127.0.0.1:*` and `ws://127.0.0.1:*` because the
  background service picks a random port; the token-protected service is the real boundary.
  `img-src` is only `'self' data: blob:`: library pictures are fetched with the token and shown from `blob:` URLs.
  `dangerousDisableAssetCspModification` is set for `style-src` only, because Tauri would otherwise
  add a nonce to styles in every HTML asset, which blocks inline styles inside the sandboxed add-on
  frames. Script hashing stays on: the add-on runtime relies on it.
  `media-src 'self' blob:` lets file previews play sound and video from `blob:` URLs the app's own script makes; nothing wider.
- **Linux `.deb`** installs are not updated from inside the app; only the AppImage is. This is the
  app's choice, not a limit of the updater: tauri-plugin-updater 2.x can install a `.deb` (it runs
  `dpkg -i` through `pkexec`, a graphical `sudo` prompt or `sudo`, and `latest.json` carries a
  `linux-x86_64-deb` entry), but that asks for an administrator password in the middle of an
  update. `update_check` and `update_install` (`src-tauri/src/main.rs`) turn it off whenever the
  app is not running from an AppImage, and Check for Updates then only looks up the newest version
  and offers its download page. There is no package repository: a `.deb` user installs the new
  `.deb` by hand.

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

## A release made on this Mac

While the pipeline is not used (it has not yet published a release), a release is made with one
command. It builds macOS only, with the same scripts the pipeline
uses, and sends nothing anywhere until it is asked to:

```bash
node scripts/release/local.mjs             # build, sign, assemble, check. Nothing leaves this Mac.
node scripts/release/local.mjs --draft     # push main and the tag, attach the files to a DRAFT
node scripts/release/local.mjs --publish   # make the draft public: the update reaches every copy
```

Before it:

1. Set the version and regenerate the kit history (checklist steps 2 and 3).
2. Write the version's entry in `docs/CHANGELOG.md` and `docs/ar/CHANGELOG.md` (`## X.Y.Z`, then
   the entry). It becomes the release page, and the new version's own Help › What's new in each
   language (the update dialog of 0.1.0 and later shows only the app's own words, never the
   unsigned notes in `latest.json`; older copies still show those). The command refuses to build
   without both.
3. Commit, on `main`.

**What it checks before building:** first, that `node_modules` holds exactly what
`package-lock.json` lists (every package at its locked version, read from its own folder;
optional packages for other platforms may be absent; nothing left over that the lock file no
longer lists). If not, it stops at once and asks you to run `npm ci`: it never installs anything
itself. Then a clean tree on `main`; a version that is not tagged on
another commit; the updater's private key and its password; `src-tauri/updater.pub`, and that
the private key really belongs to it (it signs a few bytes and checks them); both changelog
entries; the kit history; the tests — `cargo test -p thoughtsd`, `cargo test -p thoughts`, `npm run check`
(svelte-check), `npx vitest run` (the UI's own tests) and the release scripts' tests
(`--skip-tests` leaves them out, and the summary says so).

**Whose commits go into it.** Until the rulesets for `main` and the `v*` tags are on, a
collaborator can push to `main` (see
[Repository settings](#repository-settings-worth-switching-on)), so before it builds the
command lists every commit since the last release that you did not make, and refuses to build
until you have read them and run it again with `--include-others`. A commit is yours when its
author and its committer are both one of your addresses: this Mac's git identity, the addresses
in `RELEASE_OWN_EMAILS` (comma-separated), and the address GitHub makes for the repository's
owner (`<number>+<owner>@users.noreply.github.com`, which the rewritten history uses). With no
earlier release tag (0.7.1, the first release of the public repository) "since the last release"
is the whole history; only the commits of other identities are listed then, which are
Dependabot's and any made on GitHub's website (their committer is GitHub). It also refuses while GitHub's `main` has commits this Mac does not have:
someone pushed, and you have not looked yet. The summary of a build says which it was: every
commit yours, or how many were not.

**What it builds:** the universal `.dmg`, the `.app.tar.gz` with its `.sig`, and `latest.json`,
into `release-out/` and `release-meta/`. The private key is handed to the packing step only,
never to the compile step. On this Mac both steps run on the same account, so that only keeps
the key out of the compiler's environment: see
[Keeping the updater key safe](#keeping-the-updater-key-safe) for the rest. Before anything
counts as built, every update file is checked the way an installed copy will check it: against
`updater.pub`, and for exactly this version.

**`--draft`** reuses those files when they were built from the commit that is checked out. It
switches the CI, Release and Advisories workflows off, pushes `main` and the tag, attaches the
files to a draft release, and switches the workflows back on. (A push would otherwise start
CI, and the tag would start the pipeline's own release.) Nobody is offered a draft.

**`--publish`** makes the draft public. From that moment every copy that checks is offered
the update. A published release is never changed: to fix something, release a new version.

### Keeping the updater key safe

Whoever has the updater key **and** its password can ship an update that every installed copy
accepts. The key file (`~/.tauri/thoughts-updater.key`) is only usable with the password, so
where the password lives matters most.

**Where the command looks for the password**, in this order:

1. `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, if it is set. It works, with a warning: never keep it
   in a shell profile or any other file, where every program running as you can read it.
2. A separate keychain, when `THOUGHTS_UPDATER_KEYCHAIN` names one. The command unlocks it
   (macOS asks for that keychain's password in the terminal), reads the item
   `thoughts-updater`, and locks it again at once, before anything is built.
3. An **always-ask** item in the login keychain: service `thoughts-updater`, account
   `always-ask`, made with `-T ""`. macOS shows a dialog every time it is read. Choose
   **Allow**, never **Always Allow**: that would make it readable without a question again. If
   you choose **Deny**, the command stops; it never falls back to another item.
4. The item made the old way (`security add-generic-password -s thoughts-updater -a release -w`,
   the instruction before 0.1.0). It still works, with a warning on every run: an item made
   without `-T` trusts the `security` tool itself, so **any program running as you reads it
   without being asked**, with the very command this script uses. That includes the build code
   of every Rust and npm dependency, and every command an assistant runs (Thoughts' own chats run
   Claude Code with a shell). Move it.

**Moving to an always-ask item** (the simplest). Type these yourself, in Terminal:

```bash
# 1. The new item. -T "" trusts no program, so macOS asks every time; -w last asks for the
#    password here, so it never appears on a command line.
security add-generic-password -s thoughts-updater -a always-ask -T "" -w
# 2. Check: macOS asks (choose Allow), then the build signs as before.
node scripts/release/local.mjs
# 3. Delete the old item from the login keychain.
security delete-generic-password -s thoughts-updater -a release ~/Library/Keychains/login.keychain-db
```

While the old item exists the command warns that it is still there.

**Or a separate keychain that stays locked.** Its own password, locked when the Mac sleeps and
after five minutes; the command unlocks it only for the moment it reads the item:

```bash
# 1. The keychain (asks for a new password for it: use one you do not use anywhere else).
security create-keychain ~/Library/Keychains/thoughts-release.keychain-db
security set-keychain-settings -l -u -t 300 ~/Library/Keychains/thoughts-release.keychain-db
# 2. The item. `security` only asks for the password when -w comes last, and a keychain named
#    on the command line would have to come after it, so make the new keychain the default for
#    this one command, then put the login keychain back.
security default-keychain -s ~/Library/Keychains/thoughts-release.keychain-db
security add-generic-password -s thoughts-updater -a release -T "" -w
security default-keychain -s ~/Library/Keychains/login.keychain-db
security default-keychain   # must name login.keychain-db again
security lock-keychain ~/Library/Keychains/thoughts-release.keychain-db
# 3. Release with it named (or export it in your shell profile: it is a path, not a secret).
THOUGHTS_UPDATER_KEYCHAIN=~/Library/Keychains/thoughts-release.keychain-db node scripts/release/local.mjs
# 4. Delete the old item from the login keychain.
security delete-generic-password -s thoughts-updater -a release ~/Library/Keychains/login.keychain-db
```

With `-T ""` the item also asks before it is read while the keychain is unlocked, so a program
that catches the short unlocked moment still gets a dialog, not the password.

**Better still: keep the key off the account you use every day.** Both moves above stop a
silent read; they do not stop a program running as you from waiting for you to click Allow, or
from reading what you type into a terminal. The account you work, browse and run assistant
chats in is the one most likely to run something hostile. Keep the key file and its password
on a separate macOS user that is used only for releasing (a fresh clone of the repository, `gh`
signed in there, nothing else installed), or on a machine used only for that. Tauri's updater
key cannot live on a hardware token, so a separate account is the practical choice. Whichever
you choose, keep the backup copy of the key and its password in a password manager and offline,
as [the updater](#the-updater) section says.

## Rehearsing an update

`node scripts/release/rehearse-update.mjs` runs a whole in-app update on this Mac without the
installed app, the real key or GitHub. Run it before any release that changes update code
(the update commands in `src-tauri/src/main.rs`, `ui/src/lib/updates.ts`, the scripts in
`scripts/release/`).

It copies the working tree, builds the app twice, as 0.0.90 and 0.0.91, under the name
"Thoughts Rehearsal" and with a throwaway key, serves a pretend public address from this
computer, and for each scenario installs 0.0.90 in the Applications folder,
starts it and reads what happened:

| Scenario | Expected |
|---|---|
| The public address answers | 0.0.90 becomes 0.0.91 |
| Nothing published ("not found") | "No published version"; 0.0.90 stays |
| No answer at all | "Could not reach"; 0.0.90 stays |
| One byte of the download changed | Refused; 0.0.90 stays |
| Opened from a disk image | The update fails cleanly |

A rehearsal build is the only kind that installs an update without being asked. That lives
behind the `rehearsal` cargo feature, which nothing but this script switches on. Such a build
also saves a keychain entry of its own (`thoughts-rehearsal`) with the old version and reads it
with the new one, which is how the report can say whether macOS asks again after an update.

A window named "Thoughts Rehearsal" opens and closes while it runs. If macOS asks anything,
that is a finding. `--keep` leaves the work folder with `REPORT.md` and `results.json`. It
builds into the repository's own `target/` folder, so the next ordinary `cargo build` compiles
the app's own two crates once more.

Not covered, check by hand: the microphone after an update, an update while a chat is running,
a copy downloaded through a browser (the quarantine mark), an Intel Mac.

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

Every release needs its entry in `docs/CHANGELOG.md` and `docs/ar/CHANGELOG.md` before it is
built (`## X.Y.Z`, then the entry): it becomes the new version's Help › What's new in each
language and the first text of the release page (copies before 0.1.0 also show it in their update
dialog).

While the pipeline cannot run, steps 4 to 7 and 9 are done by
[`scripts/release/local.mjs`](#a-release-made-on-this-mac) instead. Steps 1 to 3 and 8 stay as
they are (for step 1, the tests are run on this Mac by that command).

1. Make sure `main` is green in CI.
2. Set the version: `node scripts/set-version.mjs X.Y.Z`, then `cargo check -p thoughts`.
   Then, **before tagging**, regenerate `crates/thoughtsd/src/kit_history.rs`: `git fetch --tags`,
   then `node scripts/kit-history.mjs`. It lists every starter file each earlier release shipped,
   so a library made by an older version is not asked for trust again because this release
   reworded one of the kit's routines. (`--check` only says whether the file is current;
   `--before X.Y.Z` leaves out that release's own tag and anything newer.) The releases up to
   and including 0.7.0 are kept in the file itself: their tags were deleted when the repository
   became public, so the script reads those entries back from the committed file (checked
   against a fingerprint, `FROZEN_SHA256`; it refuses to go on if one was changed or lost) and
   adds only the releases tagged after 0.7.0 from git. Never edit or regenerate that part by
   hand. For 0.7.1 there is no such tag, so the file stays as it is. The release workflow's
   preflight runs `node scripts/kit-history.mjs --check --before X.Y.Z` with every tag fetched and
   stops before building when the file is out of date.
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
9. Read the release notes (the draft starts with the version's entry in `docs/CHANGELOG.md`).
   While the builds are unsigned, they must send people installing by hand to the README's
   [Opening Thoughts the first time](../README.md#opening-thoughts-the-first-time) (on macOS 15
   and later, Control-click › Open no longer gets past the warning). Then press **Publish release**. From that moment the release is final: the workflow will not
   touch it again.

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
   Keep the private key and its password in a password manager, and one more copy offline.
   Where the copy you release with lives, and where its password is kept, is in
   [Keeping the updater key safe](#keeping-the-updater-key-safe).
2. Commit the public key as `src-tauri/updater.pub`
   (`cp ~/.tauri/thoughts-updater.key.pub src-tauri/updater.pub`). It is not secret. Every
   build with the updater reads it from there, on this Mac and in CI, so the two cannot
   disagree: a copy built with the wrong public key can never be updated. This is all a
   [release made on this Mac](#a-release-made-on-this-mac) needs.
3. For the pipeline, on GitHub, also add:

   | Kind | Name | Value |
   |---|---|---|
   | Secret | `TAURI_SIGNING_PRIVATE_KEY` | The whole content of `thoughts-updater.key` |
   | Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password you chose |
   | Variable | `UPDATER_PUBKEY` | The whole content of `thoughts-updater.key.pub` |

   The public key is not secret, which is why it is a *variable*, not a secret. The workflow
   still decides from this variable whether the updater is on. It must hold the same key as
   the committed file: the build stops when the two differ.
4. On the next tag the workflow:
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
      "endpoints": ["https://github.com/<owner>/<repo>/releases/latest/download/latest.json"],
      "requireSignedVersion": true
    }
  }
}
```

`scripts/release-config.mjs` generates exactly this into `src-tauri/release.conf.json` (a
build-time file, do not commit it), for the pipeline and for the local release command alike,
so that ordinary local builds stay updater-free. `requireSignedVersion` makes an installed copy
refuse an update whose signature was not made for the version the address announces, so an old
release cannot be served again under a new number. The endpoint is this repository's
"latest release" on GitHub. If you would rather hard-code it, the same block can go into
`src-tauri/tauri.conf.json`; then every build, local ones included, needs the private key in
`TAURI_SIGNING_PRIVATE_KEY` to finish.

**How it behaves in the app.** If the build has no public key or no endpoint, the updater is
not loaded: "Check for Updates…" then asks GitHub for the latest release number and offers its
download page (a Linux package says its updates come from the package manager). If it has both,
the app checks `latest.json`, and installs only an update whose signature matches the public key.

A check asks the public address, with no sign-in, and nothing else. When it answers "not found"
(nothing published yet), the app says that no published version was found. No connection, a
time-out or any other error is said as such. (0.0.12 to 0.7.0 also had a GitHub fallback through
the `gh` sign-in, switched on from a terminal, for the time the repository was private. 0.7.1
removed it; `thoughts updates …` in a terminal now only says that no step is needed, and a
switch file an older copy left in the app's folder, `updates-gh.json`, is ignored.)

The app asks about updates by itself only while "Check for new versions automatically" is on
(Settings › Maintenance, off unless switched on): once a couple of minutes after it opens, then
once a day. It never installs without the person agreeing.

Things to know:

- "Latest release" on GitHub means the latest *published*, non-prerelease release. Drafts are
  never offered to users. Publishing the draft is what rolls the update out. The addresses in
  `latest.json` (`…/releases/download/vX.Y.Z/<file>`) only exist once the draft is published.
- At the public address the updater downloads from this repository's releases without signing
  in, which only works because the repository is public. Making it private again would stop
  every copy from 0.7.1 on from finding updates.
- The first version that contains the public key cannot be reached by the updater from older
  versions. People install that one by hand; from then on updates are automatic.
- On Linux only the AppImage updates itself. `.deb` users install the new `.deb` by hand (the
  updater could install it, but the app leaves that off; see Pipeline notes).
- An unsigned macOS app does update itself. Rehearsed on macOS 26.3 on 2026-11-16: a copy in
  the Applications folder replaced itself with no password and no question, and the new version
  opened. What macOS does do is ask again for whatever is tied to the app's identity, because
  that identity changes with every unsigned build: after the update it asked the person before
  it let the new version read a keychain entry the old one had saved. The same is to be
  expected for the microphone and speech permissions (not rehearsed). A signing certificate
  ends this, and a self-made one may be enough: `rehearse-update.mjs --identity "<name>"`
  shows what it changes.

### Older copies and the public repository

How a copy that is already installed finds an update, so that the history rewrite of 2026-11
(every old release and tag deleted, one new release 0.7.1) still reaches it. Every copy from
0.0.12 to 0.7.0 was built with the same updater key (`src-tauri/updater.pub`, unchanged since
0.0.12), the one address
`https://github.com/abdulmalik-alzeer/thoughts/releases/latest/download/latest.json`,
`requireSignedVersion`, and `tauri-plugin-updater` 2.14.1. Releases before 0.0.12 have no updater.

1. It asks that address with no sign-in. GitHub forwards `releases/latest/download/<file>` to the
   newest published, non-draft, non-prerelease release's file of that name, so only the release
   marked "latest" counts; no API call, no tag list, no older release is read.
2. If the answer is a `latest.json` (200), the updater compares its `version` with its own
   (semver: a higher version is an update), picks `platforms["darwin-<arch>-app"]`, else
   `platforms["darwin-<arch>"]` (`<arch>` is `aarch64` or `x86_64`), downloads that entry's `url`
   with no sign-in, and installs it only if `signature` verifies against the key **and** the
   signature's trusted comment names exactly that version. 0.1.0 and later show the app's own
   words and a link to `https://github.com/<owner>/<repo>/releases/tag/v<version>`; 0.0.12 to
   0.0.14 show `notes` (or `notes_ar`) from `latest.json`.
3. Only if the address answers "not found" (no release published, or a private repository) does
   a 0.0.12–0.7.0 copy go on: with the fallback switched off it says "no published version";
   with it switched on it signs in with `gh` and reads the newest release through the API. Once
   0.7.1 is public step 1 answers, so the fallback is never reached, switched on or not.

So the one new release must: be published (not a draft, not a pre-release) and marked latest;
be tagged `v0.7.1`; carry an asset named exactly `latest.json` with `version` `0.7.1`,
`platforms` entries `darwin-aarch64`, `darwin-aarch64-app`, `darwin-x86_64`,
`darwin-x86_64-app` (the local release command writes these, plus `darwin-universal`), each with
the URL of the release's `.app.tar.gz` and its `.sig` content; and have that `.app.tar.gz` signed
with the same key, for version 0.7.1 (the local release command checks both). Between deleting
the old releases and publishing 0.7.1, older copies are told that no published version was found;
nothing breaks. A copy of 0.7.1 or later never asks anything else.

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
- Gatekeeper on a downloaded copy: unsigned today, so it follows the README's
  [Opening Thoughts the first time](../README.md#opening-thoughts-the-first-time) (System Settings ›
  Privacy & Security › Open Anyway on macOS 15 and later; Control-click › Open before); after
  signing, `spctl -a -vv` reports a notarised Developer ID.
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
- **Apple and Windows signing have never been exercised.** No certificate exists, and the
  signing and notarisation steps in the workflow have never run.
- **The updater, as far as it has been seen.** On 2026-11-16 (macOS 26.3, Apple Silicon) a
  rehearsal with a throwaway key and a pretend GitHub on this Mac showed: an update from the
  public address and one through the GitHub fallback (removed in 0.7.1) both installed and restarted as the new
  version; the background service was the new version afterwards; the sign-in went to the API
  only; with the fallback off nothing was asked and `gh` was not started; no answer from the
  public address did not lead to the fallback; a download with one changed byte was refused; a
  copy opened from a disk image was told to move first. Against the real GitHub only the
  requests themselves were tried, with the owner's account: the latest release, the forward to
  the file host, the bytes. **Never seen:** an update from a real published release; a
  collaborator's account; the update dialog clicked by a person
  (`rehearse-update.mjs --by-hand`); the updater key with a password, and the local
  release command past its checks; the pipeline's signing and `latest.json` steps; the
  microphone permission after an update; an update while a chat is running; a copy downloaded
  through a browser; an Intel Mac; Windows and Linux.
- **0.7.1 and the public repository.** Nobody has yet updated an installed 0.6.0, 0.7.0 or
  0.0.12 copy to 0.7.1 from the public Releases page, with or without the old fallback switched
  on, and the update rehearsal has not been run since its fallback scenarios were removed.
- **Plugin frames under the bundled CSP.** In bundled builds Tauri adds a nonce to `<style>` in
  HTML assets, which may block inline styles inside add-on and view frames. This cannot be seen
  in `tauri dev`; it needs a look in an installed build (test plan step 8).
- **The compile and sign jobs apart (0.1.0).** `release.yml` now compiles in one job and packs
  and signs in another, on a fresh machine, with the executable handed over as an artifact. That
  has never run. Its scripts are tested (`release.test.mjs` also checks the workflow's shape:
  which job gets which secret, what the sign job runs, the pinned actions) and on this Mac
  `tauri bundle` was seen packing an `.app` from nothing but the executable; whether Windows and
  Linux need anything more from the compile job, and the artifact hand-over itself, are only
  proven by the first rehearsal.
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
| `UPDATER_PUBKEY` | Variable | Updater, in the pipeline (must equal `src-tauri/updater.pub`) |

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

**Where they stand (2026-11-16, before the repository became public).** Switched on: release
immutability, Dependabot alerts and security updates, and the rule that actions are pinned to a
full commit. Already as they should be: a workflow's own token can only read, workflows cannot
approve pull requests, and no secret or variable is stored on GitHub. Check them again after the
repository was made public and its history rewritten. **To be switched on, now that the
repository is public:** the rulesets for `main` and for the release tags (below), on: ____
(the lead fills in the date).

- **Rules for `main` and for the release tags.** The repository is public, so rulesets are
  available on the free plan (before, they needed GitHub Pro). Both were **switched on 2026-11-24** ("main: only the owner", "release tags: only the owner"), together with secret scanning with push protection, private vulnerability reporting, and approval for every outside contributor's workflow run.
  Until they are, everyone with write access (on a repository owned by a personal account, every
  collaborator) can push to `main`, move or create a `v*` tag, and create or delete a release.
  What holds either way: an installed copy only installs what the updater key signed, and that
  key is on the owner's Mac and nowhere on GitHub; a published release can no longer be changed
  (immutability, below). The release command also lists every commit since the last release
  that the person releasing did not make, and waits until they have been read (see
  [A release made on this Mac](#a-release-made-on-this-mac)). Add two rulesets, each with the
  repository admin as the only one who may bypass: for the branch `main`, restrict updates and
  deletions and block force pushes (switch this on only after the rewritten history has been
  pushed, or the admin's bypass has to be used for it); for tags matching `v*`, restrict
  creations, updates and deletions.
- **Pull requests from forks.** Anyone can now open a pull request, and `ci.yml` runs on it
  with a read-only token and no secret. Under Settings → Actions → General, keep "Require
  approval for all outside collaborators" (or at least first-time contributors) so a stranger's
  workflow change does not run unseen.
- **Keep the updater key off GitHub while anyone else can push.** A workflow on any branch can
  read the repository's secrets, and anyone with write access can push such a workflow. The
  protected environment described at the end of this list is what makes it safe to store.

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
  GitHub then asks a person to approve before a job may read the secrets. On a public
  repository both environment secrets and required reviewers are free (for a private one they
  needed a paid plan). Limiting the environment to tags
  `v*` (also possible on Pro or Team) already keeps the secrets away from every branch and
  rehearsal run. To adopt it: create the environment, move the signing secrets into it, and add
  `environment: release` to the `sign` job in `release.yml` (the only job that reads them). With a tags-only rule a rehearsal
  on `main` would then be refused, so let only release runs name the environment. Worth doing
  before the updater key exists, because that key can ship code to every installed copy.
