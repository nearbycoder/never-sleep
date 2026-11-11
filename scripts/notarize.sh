#!/bin/bash
# Submit the distribution container to Apple, then staple its ticket and the app's.
# Credentials stay in Keychain; never store them in this repository.
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"

package="${1:-$project_dir/dist/Never Sleep.dmg}"
profile="${NOTARY_PROFILE:-}"
if [[ -z "$profile" ]]; then
  echo 'Set NOTARY_PROFILE to your notarytool Keychain profile name.' >&2
  echo 'Create it interactively: xcrun notarytool store-credentials "never-sleep-notary"' >&2
  exit 2
fi
if [[ ! -e "$package" ]]; then
  echo "Installer not found: $package" >&2
  exit 2
fi
case "$package" in
  *.pkg)
    signature="$(/usr/sbin/pkgutil --check-signature "$package")"
    if [[ "$signature" != *"Developer ID Installer:"* ]]; then
      echo 'PKG notarization requires a local Developer ID Installer signature.' >&2
      exit 2
    fi
    ;;
  *.dmg|*.app) ;;
  *) echo 'Expected an .app, .dmg, or Developer ID Installer-signed .pkg.' >&2; exit 2 ;;
esac
/usr/bin/codesign --verify --deep --strict "dist/Never Sleep.app"
app_signature="$(/usr/bin/codesign -dvv "dist/Never Sleep.app" 2>&1)"
if [[ "$app_signature" != *"Authority=Developer ID Application:"* ]]; then
  echo 'Sign the application with Developer ID before notarizing.' >&2
  exit 2
fi
mkdir -p target/notarization
submission="$package"
if [[ "$package" == *.app ]]; then
  submission="target/notarization/NeverSleep.zip"
  ditto -c -k --keepParent "$package" "$submission"
fi
xcrun notarytool submit "$submission" --keychain-profile "$profile" \
  --output-format json > target/notarization/submission.json
submission_id="$(/usr/bin/plutil -extract id raw -o - target/notarization/submission.json)"
echo "Apple submission: $submission_id"
echo 'Waiting for Apple. If interrupted, resume with notarytool wait using this ID.'
xcrun notarytool wait "$submission_id" --keychain-profile "$profile" \
  --output-format json > target/notarization/result.json
status="$(/usr/bin/plutil -extract status raw -o - target/notarization/result.json)"
if [[ "$status" != "Accepted" ]]; then
  xcrun notarytool log "$submission_id" --keychain-profile "$profile" \
    target/notarization/log.json
  echo "Notarization status: $status. See target/notarization/log.json." >&2
  exit 1
fi
xcrun stapler staple "$package"
xcrun stapler validate "$package"
# Apple processes the app inside the submitted installer as well.
xcrun stapler staple "dist/Never Sleep.app"
xcrun stapler validate "dist/Never Sleep.app"
if [[ "$package" == *.pkg ]]; then
  spctl --assess --type install --verbose=2 "$package"
fi
spctl --assess --type execute --verbose=2 "dist/Never Sleep.app"
echo "Notarized and stapled: $package"
