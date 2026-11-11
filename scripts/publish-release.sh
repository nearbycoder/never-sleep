#!/bin/bash
# Publish a prepared, notarized release from the Mac that owns the signing keys.
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
dry_run=false
if [[ "${1:-}" == "--dry-run" ]]; then dry_run=true; shift; fi
if [[ $# -ne 2 ]]; then
  echo 'Usage: scripts/publish-release.sh [--dry-run] VERSION RELEASE_NOTES_FILE' >&2
  exit 2
fi
version="$1"
notes="$2"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ || ! -f "$notes" ]]; then
  echo 'Provide a stable x.y.z version and an existing release notes file.' >&2
  exit 2
fi
repo=nearbycoder/never-sleep
tag="v$version"
bundle="$project_dir/dist/Never Sleep.app"
dmg="$project_dir/dist/Never Sleep.dmg"
sparkle="$project_dir/target/sparkle/bin"
account="${SPARKLE_KEY_ACCOUNT:-never-sleep}"
head="$(git rev-parse HEAD)"
if [[ -n "$(git status --porcelain)" || "$(git branch --show-current)" != main ]]; then
  echo 'Commit all changes on main before building and publishing a release.' >&2
  exit 1
fi
git fetch origin main
if [[ "$(git rev-parse origin/main)" != "$head" ]]; then
  echo 'Push this exact commit to origin/main before publishing.' >&2
  exit 1
fi
origin="$(git remote get-url origin)"
case "$origin" in
  git@github.com:nearbycoder/never-sleep.git|git@github.com:nearbycoder/never-sleep|https://github.com/nearbycoder/never-sleep.git|https://github.com/nearbycoder/never-sleep) ;;
  *) echo 'origin must be nearbycoder/never-sleep.' >&2; exit 1 ;;
esac
gh repo view "$repo" --json nameWithOwner --jq .nameWithOwner
[[ -x "$sparkle/sign_update" && -x "$sparkle/generate_keys" && -f "$dmg" ]]
/usr/bin/codesign --verify --deep --strict "$bundle"
/usr/bin/xcrun stapler validate "$bundle"
/usr/bin/xcrun stapler validate "$dmg"
/usr/sbin/spctl --assess --type execute --verbose=2 "$bundle"
identity="$(/usr/bin/codesign -dvv "$bundle" 2>&1)"
[[ "$identity" == *'Authority=Developer ID Application:'* ]]
mkdir -p target/releases
stage="$(mktemp -d "$project_dir/target/releases/prepare.XXXXXX")"
mount="$stage/mounted"
mounted=false
cleanup() {
  status=$?
  if $mounted; then /usr/bin/hdiutil detach "$mount" || true; fi
  # Retain prepared public artifacts for inspection and retry; no secrets here.
  echo "Prepared artifacts: $stage"
  exit "$status"
}
trap cleanup EXIT
mkdir "$mount"
/usr/bin/hdiutil attach -readonly -nobrowse -mountpoint "$mount" "$dmg" > "$stage/mount.log"
mounted=true
archived="$mount/Never Sleep.app"
/usr/bin/codesign --verify --deep --strict "$archived"
/usr/bin/xcrun stapler validate "$archived"
/usr/sbin/spctl --assess --type execute --verbose=2 "$archived"
[[ "$(/usr/bin/lipo -archs "$archived/Contents/MacOS/never-sleep")" == arm64 ]]
public_key="$("$sparkle/generate_keys" --account "$account" -p)"
python3 - "$bundle" "$archived" "$version" "$head" "$public_key" <<'PY'
import hashlib, pathlib, plistlib, re, sys
local, archived = map(pathlib.Path, sys.argv[1:3])
version, revision, key = sys.argv[3:]
with (local / 'Contents/Info.plist').open('rb') as stream:
    info = plistlib.load(stream)
with (archived / 'Contents/Info.plist').open('rb') as stream:
    embedded = plistlib.load(stream)
assert info == embedded, 'The disk image contains a different app build'
assert info['CFBundleShortVersionString'] == version, 'Release version differs from app'
assert info.get('NeverSleepSourceRevision') == revision, 'Rebuild from the committed source before publishing'
assert info['SUPublicEDKey'] == key, 'Sparkle signing key does not match the application'
assert info['SUFeedURL'] == 'https://nearbycoder.github.io/never-sleep/appcast.xml', 'Unexpected feed URL'
cargo = pathlib.Path('Cargo.toml').read_text().split('[dependencies]')[0]
assert re.search(r'^version = "' + re.escape(version) + r'"$', cargo, re.M), 'Cargo version differs from app'
for relative in ('Contents/MacOS/never-sleep', 'Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle'):
    assert hashlib.sha256((local / relative).read_bytes()).digest() == hashlib.sha256((archived / relative).read_bytes()).digest(), 'The disk image contains different executable code'
PY
/usr/bin/hdiutil detach "$mount"
mounted=false
/bin/cp "$dmg" "$stage/Never-Sleep.dmg"
signature="$("$sparkle/sign_update" --account "$account" -p "$stage/Never-Sleep.dmg")"
"$sparkle/sign_update" --account "$account" --verify "$stage/Never-Sleep.dmg" "$signature"
python3 scripts/generate-appcast.py generate \
  --plist "$bundle/Contents/Info.plist" --archive "$stage/Never-Sleep.dmg" \
  --signature "$signature" --output "$stage/appcast.xml"
"$sparkle/sign_update" --account "$account" "$stage/appcast.xml"
"$sparkle/sign_update" --account "$account" --verify "$stage/appcast.xml"
python3 - "$stage" <<'PY'
import hashlib, pathlib, sys
root = pathlib.Path(sys.argv[1])
lines = [f'{hashlib.sha256((root / name).read_bytes()).hexdigest()}  {name}\n' for name in ('Never-Sleep.dmg', 'appcast.xml')]
(root / 'SHA256SUMS').write_text(''.join(lines))
PY
# A monotonically increasing build number prevents accidental downgrade feeds.
python3 scripts/generate-appcast.py fetch-latest --directory "$stage/previous"
python3 - "$stage" <<'PY'
import pathlib, sys, xml.etree.ElementTree as ET
root = pathlib.Path(sys.argv[1]); ns = {'s': 'http://www.andymatuschak.org/xml-namespaces/sparkle'}
previous = ET.parse(root / 'previous/appcast.xml').findtext('channel/item/s:version', namespaces=ns)
current = ET.parse(root / 'appcast.xml').findtext('channel/item/s:version', namespaces=ns)
assert previous is None or int(current) > int(previous), 'CFBundleVersion must increase beyond the latest published release'
PY
if $dry_run; then
  echo "Preflight passed for $tag at $head. No GitHub release was created."
  exit 0
fi
if gh release view "$tag" --repo "$repo" --json isDraft,targetCommitish > "$stage/existing.json" 2>/dev/null; then
  python3 - "$stage/existing.json" "$head" <<'PY'
import json, sys
release = json.load(open(sys.argv[1]))
assert release['isDraft'] and release['targetCommitish'] == sys.argv[2], 'Refusing to replace a public release or a draft from a different commit'
PY
else
  gh release create "$tag" --repo "$repo" --target "$head" --draft \
    --title "Never Sleep $version" --notes-file "$notes"
fi
gh release upload "$tag" --repo "$repo" --clobber \
  "$stage/Never-Sleep.dmg" "$stage/appcast.xml" "$stage/SHA256SUMS"
gh release edit "$tag" --repo "$repo" --draft=false --latest --notes-file "$notes"
# Release events trigger Pages with the normal local gh user token. Dispatch also
# covers publication made with a GITHUB_TOKEN, whose events do not trigger CI.
gh workflow run pages.yml --repo "$repo" --ref main
gh release view "$tag" --repo "$repo" --json url --jq .url
