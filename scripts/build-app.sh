#!/bin/bash
# Build a signed macOS application. No installation or system changes.
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"

profile="release"
build_args=(--locked --bin never-sleep --release)
case "${1:-}" in
  "") ;;
  --debug) profile="debug"; build_args=(--locked --bin never-sleep) ;;
  -h|--help)
    echo "Usage: scripts/build-app.sh [--debug]"
    echo "Creates dist/Never Sleep.app for this Mac; defaults to a release build."
    exit 0
    ;;
  *) echo "Unknown option: $1" >&2; exit 2 ;;
esac
if [[ $# -gt 1 ]]; then
  echo "Usage: scripts/build-app.sh [--debug]" >&2
  exit 2
fi
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Building Never Sleep.app requires macOS and Xcode command-line tools." >&2
  exit 1
fi

# Matches Info.plist; Rune's Metal renderer requires at least macOS 13.
export MACOSX_DEPLOYMENT_TARGET=14.0
target_dir="$(cargo metadata --locked --format-version 1 --no-deps | /usr/bin/plutil -extract target_directory raw -o - -)"
binary_dir="$target_dir/$profile"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  case "$CARGO_BUILD_TARGET" in
    aarch64-apple-darwin|x86_64-apple-darwin) ;;
    *) echo "CARGO_BUILD_TARGET must be a macOS target triple." >&2; exit 1 ;;
  esac
  binary_dir="$target_dir/$CARGO_BUILD_TARGET/$profile"
fi

./scripts/fetch-sparkle.sh
cargo build "${build_args[@]}"
icon_dir="$target_dir/app-icon"
/usr/bin/xcrun swift scripts/make-icon.swift "$icon_dir"
/usr/bin/iconutil --convert icns --output "$icon_dir/NeverSleep.icns" "$icon_dir/NeverSleep.iconset"

bundle="$project_dir/dist/Never Sleep.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
# Replace only this app's known files. Do not recursively delete an existing bundle.
/usr/bin/install -m 755 "$binary_dir/never-sleep" "$bundle/Contents/MacOS/never-sleep"
/usr/bin/install -m 644 packaging/Info.plist "$bundle/Contents/Info.plist"
/usr/bin/install -m 644 "$icon_dir/NeverSleep.icns" "$bundle/Contents/Resources/NeverSleep.icns"
/usr/bin/plutil -insert NeverSleepSourceRevision -string "$(git rev-parse HEAD)" "$bundle/Contents/Info.plist"
mkdir -p "$bundle/Contents/Frameworks"
/usr/bin/ditto target/sparkle/Sparkle.framework "$bundle/Contents/Frameworks/Sparkle.framework"
/usr/bin/install -m 644 target/sparkle/LICENSE "$bundle/Contents/Resources/Sparkle-LICENSE.txt"
/usr/bin/install -m 644 LICENSE "$bundle/Contents/Resources/LICENSE.txt"
/usr/bin/install -m 644 packaging/licenses/Rune-LICENSE.txt "$bundle/Contents/Resources/Rune-LICENSE.txt"
/usr/bin/plutil -lint "$bundle/Contents/Info.plist"
sign_identity="${SIGN_IDENTITY:-}"
if [[ -z "$sign_identity" ]]; then
  sign_identity="$(security find-identity -v -p codesigning | /usr/bin/awk '/"Developer ID Application:/ { print $2; exit }')"
fi
if [[ -n "$sign_identity" && "$sign_identity" != "-" ]]; then
  ./scripts/sign-bundle.sh "$bundle" "$sign_identity"
  echo "Signed with identity: $sign_identity (hardened runtime + secure timestamp)"
else
  ./scripts/sign-bundle.sh "$bundle" -
  echo "Ad-hoc signed for local use; no Developer ID Application identity available."
fi
/usr/bin/codesign --verify --strict "$bundle"
echo "Built: $bundle"
echo "Launch: open \"$bundle\""
