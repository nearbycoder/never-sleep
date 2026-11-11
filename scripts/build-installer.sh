#!/bin/bash
# Package the already-built app in Apple's native Installer. Never installs it.
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"

case "${1:-}" in
  "") ;;
  -h|--help)
    echo "Usage: scripts/build-installer.sh"
    echo "Packages dist/Never Sleep.app into dist/Never Sleep Installer.pkg."
    echo "Set INSTALLER_SIGN_IDENTITY to a Developer ID Installer identity to sign."
    exit 0
    ;;
  *) echo "Usage: scripts/build-installer.sh" >&2; exit 2 ;;
esac
if [[ $# -gt 1 ]]; then
  echo "Usage: scripts/build-installer.sh" >&2
  exit 2
fi
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Building the installer requires macOS and Xcode command-line tools." >&2
  exit 1
fi

bundle="$project_dir/dist/Never Sleep.app"
plist="$bundle/Contents/Info.plist"
if [[ ! -f "$plist" ]]; then
  echo "Build the app first with scripts/build-app.sh." >&2
  exit 1
fi
/usr/bin/codesign --verify --strict "$bundle"
version="$(/usr/bin/plutil -extract CFBundleShortVersionString raw -o - "$plist")"
minimum_os="$(/usr/bin/plutil -extract LSMinimumSystemVersion raw -o - "$plist")"
executable="$(/usr/bin/plutil -extract CFBundleExecutable raw -o - "$plist")"
identifier="$(/usr/bin/plutil -extract CFBundleIdentifier raw -o - "$plist")"
if [[ "$identifier" != "com.nearby.never-sleep" || "$executable" != "never-sleep" ]]; then
  echo "The bundle must be the Never Sleep application." >&2
  exit 1
fi
if [[ ! "$version" =~ ^[0-9]+(\.[0-9]+)*$ || ! "$minimum_os" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
  echo "App version and minimum macOS version must be numeric version strings." >&2
  exit 1
fi
# Never advertise support below the app's packaging baseline.
minimum_major="${minimum_os%%.*}"
if (( minimum_major < 14 )); then minimum_os="14.0"; fi
architectures="$(/usr/bin/lipo -archs "$bundle/Contents/MacOS/$executable")"
for architecture in $architectures; do
  case "$architecture" in
    arm64|x86_64) ;;
    *) echo "Unsupported app architecture: $architecture" >&2; exit 1 ;;
  esac
done
if [[ -z "$architectures" ]]; then
  echo "The app binary has no supported architecture." >&2
  exit 1
fi
host_architectures="${architectures// /,}"

mkdir -p "$project_dir/target/installer" "$project_dir/dist"
stage="$(mktemp -d "$project_dir/target/installer/build.XXXXXX")"
# Only the fresh directory returned by mktemp is ever removed.
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/root/Applications" "$stage/packages" "$stage/resources"
/usr/bin/ditto "$bundle" "$stage/root/Applications/Never Sleep.app"
/usr/bin/ditto packaging/installer/resources "$stage/resources"
/usr/bin/sips -Z 256 assets/app-icon.png --out "$stage/app-icon.png" >/dev/null
# productbuild drops images not referenced directly by the distribution XML.
# Inline the artwork so Installer's HTML reader always receives a complete page.
/usr/bin/xcrun swift packaging/installer/embed-resources.swift "$stage/resources" "$stage/app-icon.png"
/usr/bin/sed \
  -e "s/__APP_VERSION__/$version/g" \
  -e "s/__MIN_OS__/$minimum_os/g" \
  -e "s/__ARCHITECTURES__/$host_architectures/g" \
  packaging/installer/Distribution.xml.in > "$stage/Distribution.xml"
/usr/bin/xmllint --noout "$stage/Distribution.xml"
/usr/bin/plutil -lint packaging/installer/components.plist

/usr/bin/pkgbuild \
  --root "$stage/root" \
  --component-plist packaging/installer/components.plist \
  --identifier com.nearby.never-sleep.pkg \
  --version "$version" \
  --install-location / \
  --ownership recommended \
  "$stage/packages/never-sleep-component.pkg"

product_args=(
  --distribution "$stage/Distribution.xml"
  --package-path "$stage/packages"
  --resources "$stage/resources"
  --identifier com.nearby.never-sleep.installer
  --version "$version"
)
if [[ -n "${INSTALLER_SIGN_IDENTITY:-}" ]]; then
  product_args+=(--sign "$INSTALLER_SIGN_IDENTITY" --timestamp)
fi
/usr/bin/productbuild "${product_args[@]}" "$stage/Never Sleep Installer.pkg"

# Inspect the complete product and verify the actual archived application.
# Expansion checks payload paths and preservation of the application signature.
/usr/sbin/pkgutil --expand-full "$stage/Never Sleep Installer.pkg" "$stage/expanded"
archived_app="$stage/expanded/never-sleep-component.pkg/Payload/Applications/Never Sleep.app"
if [[ ! -f "$archived_app/Contents/MacOS/never-sleep" ]]; then
  echo "Installer verification failed: app is missing from /Applications payload." >&2
  exit 1
fi
/usr/bin/codesign --verify --strict "$archived_app"
/usr/sbin/pkgutil --payload-files "$stage/Never Sleep Installer.pkg" > "$stage/payload-files.txt"
if [[ -n "${INSTALLER_SIGN_IDENTITY:-}" ]]; then
  /usr/sbin/pkgutil --check-signature "$stage/Never Sleep Installer.pkg"
fi
output="$project_dir/dist/Never Sleep Installer.pkg"
/bin/mv -f "$stage/Never Sleep Installer.pkg" "$output"
echo "Built: $output"
echo "Installs: /Applications/Never Sleep.app"
echo "Requires: macOS $minimum_os or later; $architectures"
if [[ -n "${INSTALLER_SIGN_IDENTITY:-}" ]]; then
  echo "Installer signing: signed with $INSTALLER_SIGN_IDENTITY"
else
  echo "Installer signing: unsigned (INSTALLER_SIGN_IDENTITY was not provided)."
fi
echo "Notarization: not performed by this script."
