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
  Until then "Check for Updates…" says updates are not configured in this build.

## How a release is built

Pushing a git tag that starts with `v` (for example `v0.0.2`) runs
`.github/workflows/release.yml`. It builds, on GitHub's machines:

| Platform | What you get |
|---|---|
| macOS | A universal `.dmg` (Apple Silicon + Intel). Not yet published; see above |
| Windows | An `.exe` installer (NSIS, installs for the current user, no admin prompt) |
| Linux | A `.deb` and an `.AppImage` |

and attaches them to a **draft** GitHub Release. A draft is invisible to the public. Nothing
ships until you open the draft and press **Publish**.

`.github/workflows/ci.yml` runs on every push and pull request: daemon tests, the UI build and
a compile check on macOS, plus compile checks on Windows and Linux.

## Pipeline notes

- **Job order:** `tests` (the CI workflow) → `create-release` (one draft) → `build` per platform, one
  at a time so the platforms never race on the draft or on `latest.json`. Serial builds are slower.
- **Pinned actions:** every third-party action is pinned to a full commit SHA with the tag in a
  comment; Dependabot (`.github/dependabot.yml`) proposes updates.
- **Clippy** runs but is advisory for now (it reports existing warnings).
- **Content-security policy:** `connect-src` and `img-src` keep `http://127.0.0.1:*` because the
  background service picks a random port; the token-protected service is the real boundary.
  `dangerousDisableAssetCspModification` is set for `style-src` only, because Tauri would otherwise
  add a nonce to styles in every HTML asset, which blocks inline styles inside the sandboxed add-on
  frames. Script hashing stays on: the add-on runtime relies on it.
- **Linux `.deb`** installs update through the package manager; only the AppImage self-updates.

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
3. Commit (`Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`) and push to `main`.
4. Tag and push the tag:
   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
5. Wait for the **Release** workflow (Actions tab). Each platform is a separate job; one can
   fail without stopping the others.
6. Open the draft release on GitHub. Check that every expected file is attached (and
   `latest.json` if the updater is configured).
7. Download each installer and run the [first-run test plan](#first-run-test-plan) for its
   platform. Do not publish a platform that has not passed it.
8. Write the release notes, then press **Publish release**.

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
   - builds signed update files and a `latest.json`, and attaches them to the release.

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
not loaded and the app reports "updates are not configured in this build". If it has both, the
app checks `latest.json`, and installs only an update whose signature matches the public key.

Things to know:

- "Latest release" on GitHub means the latest *published*, non-prerelease release. Drafts are
  never offered to users. Publishing the draft is what rolls the update out.
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
- The translucent window, the Recovery menu and "Undo last app change" in the native menu.
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
- **The release workflow end to end.** Draft creation from three parallel jobs, the npm and
  crate Tauri versions agreeing, and artefact names have not been confirmed on a real tag since
  the last changes.
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
- The action versions in `.github/workflows/` are pinned to release tags. For stricter supply
  chain safety, pin them to full commit hashes, and confirm `tauri-apps/tauri-action` is on a
  current release when you first run the workflow.
