# Releasing Thoughts

How a new version goes from this repository to people's computers. Read it top to bottom the
first time; after that the [checklist](#release-checklist) is enough.

**Where things stand today**

- macOS is the only platform that has been built and run. Windows and Linux builds are wired up
  in CI but **have never been tested on a real machine**. Treat the first Windows/Linux
  installers as experiments.
- Nothing is signed yet. No signing certificate, no notarisation, no updater key exists. The
  release workflow works without them and produces unsigned installers.
- The in-app updater is built in but switched off until an updater key is configured (below).
  Until then "Check for Updates…" says updates are not configured in this build.

## How a release is built

Pushing a git tag that starts with `v` (for example `v0.0.2`) runs
`.github/workflows/release.yml`. It builds, on GitHub's machines:

| Platform | What you get |
|---|---|
| macOS | A universal `.dmg` (Apple Silicon + Intel) |
| Windows | An `.exe` installer (NSIS, installs for the current user, no admin prompt) |
| Linux | A `.deb` and an `.AppImage` |

and attaches them to a **draft** GitHub Release. A draft is invisible to the public. Nothing
ships until you open the draft and press **Publish**.

`.github/workflows/ci.yml` runs on every push and pull request: daemon tests, the UI build and
a compile check on macOS, plus compile checks on Windows and Linux.

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
7. Download and try each installer by hand (see [What stays manual](#what-stays-manual)).
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
- Trying each installer on a real machine before publishing. At minimum: install over the
  previous version, open the app, send one chat message, close the window, reopen from the
  tray icon, choose **Quit Thoughts completely** and confirm nothing is left running.
- Windows and Linux in particular. The window settings, menus, tray icon, single-instance
  behaviour, the hidden-console flags for the background service, and the installers were
  written on a Mac and have not been run on those systems.
- Creating and renewing the Apple certificate (it expires after five years) and any Windows
  certificate, and keeping the updater private key backed up.
- The action versions in `.github/workflows/` are pinned to release tags. For stricter supply
  chain safety, pin them to full commit hashes, and confirm `tauri-apps/tauri-action` is on a
  current release when you first run the workflow.
