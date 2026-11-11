# Never Sleep

[Download for Mac](https://github.com/nearbycoder/never-sleep/releases/latest/download/Never-Sleep.dmg) · [Website](https://nearbycoder.github.io/never-sleep/) · [Release guide](RELEASE.md)

A native macOS utility built with [Rune](https://github.com/anomalyco/rune). Keep your Mac awake while it's plugged in, with an optional setting to keep the display awake too.

The compact settings window uses native Liquid Glass on macOS 26+, a native popover material on earlier versions, San Francisco typography, and the system accent color. Reduce Transparency replaces glass with an opaque background. The custom amber sun/power icon is embedded in the app and packaged at every macOS icon size.

## Run

Requires macOS 14 or later, Xcode Command Line Tools, and Rust 1.99.1 (pinned in `rust-toolchain.toml`).

```sh
cargo run --locked
```

To make a local app bundle with an icon:

```sh
./scripts/build-app.sh
open "dist/Never Sleep.app"
```

Drag the bundle into Applications to keep it. The script automatically uses a local Developer ID Application identity if available, or signs ad hoc for local development. Override with `SIGN_IDENTITY` when needed. It builds for the current Mac's architecture. Use `./scripts/build-app.sh --debug` for a faster development bundle.

## Signed release and installers

The release app can also use Xcode's cloud-managed Developer ID signing, with the account already signed into Xcode:

```sh
./scripts/build-app.sh
DEVELOPMENT_TEAM=YOUR_TEAM_ID ./scripts/sign-release.sh
NOTARY_PROFILE=never-sleep-notary ./scripts/notarize.sh "dist/Never Sleep.app"
./scripts/build-dmg.sh
NOTARY_PROFILE=never-sleep-notary ./scripts/notarize.sh
```

`sign-release.sh` uses your local Apple Development identity to prepare an archive, then asks Xcode to export with automatic Developer ID signing. If multiple development identities are installed, set `DEVELOPMENT_SIGN_IDENTITY` to the intended identity's name or SHA-1. Cloud signing requires the account's signing permission. Signing enables hardened runtime and a secure timestamp.

The disk image at `dist/Never Sleep.dmg` provides a branded, native Finder drag-to-Applications installation. The app is Developer ID signed; when no local Developer ID key is available, the disk image itself is unsigned, and its notarization ticket is handled separately. The notary script checks Apple's acceptance, staples the tickets, and verifies the app with Gatekeeper. No secrets are stored in the repository. To create a notarization Keychain profile, run `xcrun notarytool store-credentials "never-sleep-notary"` interactively.

A native Apple Installer package with custom welcome, information, and completion screens is also available:

```sh
INSTALLER_SIGN_IDENTITY="Developer ID Installer: Your Name (TEAM_ID)" ./scripts/build-installer.sh
NOTARY_PROFILE=never-sleep-notary ./scripts/notarize.sh "dist/Never Sleep Installer.pkg"
```

The package installs only `/Applications/Never Sleep.app`; it runs no installation scripts and does not launch the app automatically. Without `INSTALLER_SIGN_IDENTITY`, the package is explicitly unsigned and must not be presented as a notarized public release. Apple does not support cloud-managed signing for custom `.pkg` files: those require a local Developer ID Installer certificate **and its private key**. The signed app can use cloud signing independently of this limitation.

## Updates

Packaged releases include [Sparkle 2](https://sparkle-project.org/), with a native **Check for Updates…** command in the app and menu bar menus. Automatic checks and installation are enabled by default; both can be changed in the menu bar menu. Updates are installed when the app quits or through Sparkle's native prompt. The current version checks a signed HTTPS appcast on GitHub Pages and verifies each update with the embedded Ed25519 public key. No system profiling is sent. Development runs without the embedded framework disable update commands.

Version 0.2.0 is the first release with the updater. Install it once if you have the earlier 0.1.0 build. Subsequent releases can update through the app.

Publishing a GitHub release refreshes the Pages download metadata and signed feed automatically. The private update key and Apple credentials stay in Keychain on the release Mac, not GitHub Actions. See [RELEASE.md](RELEASE.md) for the full repeatable process.

## Use

- Turn on **Keep awake on power**. It prevents automatic idle sleep whenever macOS reports external power, including when the battery is fully charged.
- Turn on **Keep display awake** to also prevent display idle sleep while protection is active. This option alone does not enable protection.
- Unplugging pauses protection; reconnecting resumes it. Power is checked every two seconds, including while the window is closed. Battery, UPS, unknown power, and power-read failures release protection.
- Turn off **Show in Dock** for menu-bar-only mode. The Dock icon disappears immediately and the choice persists across launches. Turn it back on to restore the Dock icon.
- Close the window or click **Done** to keep running. Use the menu bar sun icon → **Show Never Sleep…** to reopen settings, change the power toggles, or quit. When enabled, the Dock icon also reopens the window.
- **⌘K** toggles keep-awake, **⌘W** closes the window, and **⌘Q** quits. Tab and Space operate the switches.
- The power toggles start off; **Show in Dock** starts on. Settings persist between launches in `~/Library/Application Support/com.nearby.never-sleep/prefs.json`.

No administrator privileges, helper daemon, or permanent `pmset` changes are needed. Quitting releases all assertions. Lid closure, manual Sleep, and emergency system sleep still follow macOS behavior. This app doesn't disable screen locking. It must be running for protection to work; it does not register itself to launch at login.

## Verify

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
# On a Mac: briefly acquire/release real IOKit assertions and read power.
cargo test --locked -- --ignored --nocapture
```

Tests cover unplug/replug, unknown power, assertion failure/retry, display changes, preference persistence/failure, and clicks through the real Rune interface. Hardware tests are opt-in; ordinary tests never inhibit sleep.

Rune can render the interface without opening a window:

```sh
cargo run --locked -- --snap target/active.png --tree target/active.json --scene active --theme light
cargo run --locked -- --snap target/battery.png --scene battery --theme dark --size 540x660
```

Scenes: `active`, `display`, `battery`, `off`, `unknown`. These are simulated states for UI development. Headless runs and HTML exports never create power assertions, write your settings, or add a menu bar item. Native windows always replace fixtures with live power management and your saved preferences.

For manual verification, enable protection on AC and run `pmset -g assertions`: the `never-sleep` process should own `PreventUserIdleSystemSleep` and, if selected, `PreventUserIdleDisplaySleep`. Unplug, disable protection, or quit and those assertions should disappear. A different app may still hold its own assertions.

## Implementation

- `src/ui.rs`: Rune interface, light/dark appearance, accessible animated switches.
- `src/controller.rs`: persisted preferences and testable power policy.
- `src/power.rs`: IOKit power-source reading and process-owned sleep assertions.
- `src/status_item.rs`: AppKit menu bar integration (Rune's tray API currently doesn't support macOS).
- `src/main.rs`: window lifecycle, background polling, keyboard and menu actions.
- `assets/app-icon.png`: full-resolution icon artwork, generated with the built-in image generation tool; the exact prompt is in `assets/app-icon-prompt.txt`.
- `scripts/make-icon.swift`: converts the committed artwork to all native icon sizes, preserving alpha.

Rune is pre-release; its Git revision and the dependency lockfile are pinned for reproducible builds.
