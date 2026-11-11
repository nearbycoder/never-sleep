#!/bin/bash
# Sign with the Developer ID identity managed by the account signed into Xcode.
# Useful when the Developer ID private key lives in Apple's cloud, not Keychain.
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
if [[ -z "${DEVELOPMENT_TEAM:-}" ]]; then
  echo 'Set DEVELOPMENT_TEAM to the 10-character Apple developer team ID.' >&2
  exit 2
fi
bundle="$project_dir/dist/Never Sleep.app"
if [[ ! -d "$bundle" ]]; then
  echo 'Run scripts/build-app.sh first.' >&2
  exit 2
fi
development_identity="${DEVELOPMENT_SIGN_IDENTITY:-Apple Development}"
mkdir -p target/release-signing
archive_root="$(mktemp -d "$project_dir/target/release-signing/export.XXXXXX")"
export NEVER_SLEEP_ARCHIVE_ROOT="$archive_root"
python3 - <<'PY'
from pathlib import Path
import datetime, os, plistlib, subprocess
root = Path.cwd()
work = Path(os.environ['NEVER_SLEEP_ARCHIVE_ROOT'])
archive = work / 'Never Sleep.xcarchive'
app = root / 'dist' / 'Never Sleep.app'
with (app / 'Contents' / 'Info.plist').open('rb') as stream:
    app_info = plistlib.load(stream)
destination = archive / 'Products' / 'Applications' / 'Never Sleep.app'
destination.parent.mkdir(parents=True)
subprocess.run(['/usr/bin/ditto', str(app), str(destination)], check=True)
properties = {key: app_info[key] for key in ('CFBundleIdentifier', 'CFBundleShortVersionString', 'CFBundleVersion')}
properties.update(ApplicationPath='Applications/Never Sleep.app', Team=os.environ['DEVELOPMENT_TEAM'])
info = dict(ArchiveVersion=2, Name='Never Sleep', SchemeName='Never Sleep',
            CreationDate=datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None),
            ApplicationProperties=properties)
with (archive / 'Info.plist').open('wb') as stream:
    plistlib.dump(info, stream)
with (work / 'ExportOptions.plist').open('wb') as stream:
    plistlib.dump(dict(method='developer-id', teamID=os.environ['DEVELOPMENT_TEAM'],
                      signingStyle='automatic', destination='export'), stream)
PY
# Prepare only the archive copy. A failed export must leave the current release intact.
./scripts/sign-bundle.sh "$archive_root/Never Sleep.xcarchive/Products/Applications/Never Sleep.app" "$development_identity"
xcodebuild -exportArchive \
  -archivePath "$archive_root/Never Sleep.xcarchive" \
  -exportOptionsPlist "$archive_root/ExportOptions.plist" \
  -exportPath "$archive_root/signed" -allowProvisioningUpdates
signed_app="$archive_root/signed/Never Sleep.app"
/usr/bin/codesign --verify --deep --strict "$signed_app"
signature="$(/usr/bin/codesign -dvv "$signed_app" 2>&1)"
if [[ "$signature" != *"Authority=Developer ID Application:"* ]]; then
  echo 'Xcode did not produce a Developer ID Application signature.' >&2
  exit 1
fi
/usr/bin/ditto "$signed_app" "$bundle"
/usr/bin/codesign --verify --deep --strict "$bundle"
echo "Developer ID signed: $bundle"
echo 'Notarization is a separate step. Run it after building the distribution container.'
