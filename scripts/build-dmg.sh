#!/bin/bash
# Package an already signed app. Never alters, re-signs, or notarizes the app.
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
if [[ $# -gt 0 ]]; then
  echo "Usage: scripts/build-dmg.sh"
  echo "Packages dist/Never Sleep.app into a drag-to-Applications disk image."
  [[ "$1" == "--help" || "$1" == "-h" ]] && exit 0
  exit 2
fi
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Building the disk image requires macOS and Xcode command-line tools." >&2
  exit 1
fi
bundle="$project_dir/dist/Never Sleep.app"
if [[ ! -f "$bundle/Contents/Info.plist" ]]; then
  echo "Build and sign dist/Never Sleep.app first." >&2
  exit 1
fi
/usr/bin/codesign --verify --strict "$bundle"

# Isolated, pinned build tools; no global pip installations or Finder automation.
tools_dir="$project_dir/target/dmg-tools"
if command -v uv >/dev/null 2>&1; then
  if [[ ! -x "$tools_dir/bin/python" ]]; then
    uv venv --python 3.12 "$tools_dir"
  fi
  uv pip install --python "$tools_dir/bin/python" -r packaging/dmg/requirements.txt
else
  if [[ ! -x "$tools_dir/bin/python" ]]; then
    python3 -c 'import sys; assert sys.version_info >= (3, 10), "Install uv or Python 3.10+ to build the disk image"'
    python3 -m venv "$tools_dir"
  fi
  "$tools_dir/bin/python" -m pip install -r packaging/dmg/requirements.txt
fi
build_python="$tools_dir/bin/python"
mkdir -p "$project_dir/target/dmg" "$project_dir/dist"
stage="$(mktemp -d "$project_dir/target/dmg/build.XXXXXX")"

cleanup() {
  exit_status=$?
  trap - EXIT
  # Detach only images created inside this invocation's unique staging folder,
  # including an intermediate image if dmgbuild was interrupted after mounting.
  if ! "$build_python" - "$stage" <<'PY'
import pathlib
import plistlib
import subprocess
import sys
stage = pathlib.Path(sys.argv[1]).resolve()
images = plistlib.loads(subprocess.check_output(["/usr/bin/hdiutil", "info", "-plist"]))["images"]
for image in images:
    path = pathlib.Path(image.get("image-path", "/")).resolve()
    if stage not in path.parents:
        continue
    devices = [entry["dev-entry"] for entry in image.get("system-entities", []) if "dev-entry" in entry]
    if devices:
        command = ["/usr/bin/hdiutil", "detach", devices[0]]
        if subprocess.run(command, stdout=subprocess.DEVNULL).returncode:
            subprocess.run(command + ["-force"], check=True, stdout=subprocess.DEVNULL)
PY
  then
    echo "Could not detach a staged image; kept $stage for recovery." >&2
    exit 1
  fi
  rm -rf "$stage"
  exit "$exit_status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

/usr/bin/xcrun swift packaging/dmg/make-background.swift "$stage"
"$tools_dir/bin/dmgbuild" \
  -s packaging/dmg/settings.py \
  -D "app=$bundle" \
  -D "background=$stage/background.png" \
  "Never Sleep" "$stage/Never Sleep.dmg"
/usr/bin/hdiutil verify "$stage/Never Sleep.dmg"

mkdir "$stage/verify"
/usr/bin/hdiutil attach -readonly -nobrowse -mountpoint "$stage/verify" \
  "$stage/Never Sleep.dmg" > "$stage/attach.log"
/usr/bin/codesign --verify --strict "$stage/verify/Never Sleep.app"
# A stapled source must retain its ticket through the copy into the image.
if /usr/bin/xcrun stapler validate "$bundle" > "$stage/source-stapler.log" 2>&1; then
  /usr/bin/xcrun stapler validate "$stage/verify/Never Sleep.app"
fi
"$build_python" - "$stage/verify" "$stage/Never Sleep.dmg" <<'PY'
import os
from pathlib import Path
import plistlib
import subprocess
import sys
from ds_store import DSStore
root = Path(sys.argv[1])
assert sorted(item.name for item in root.iterdir() if not item.name.startswith('.')) == ['Applications', 'Never Sleep.app']
assert (root / 'Applications').is_symlink() and os.readlink(root / 'Applications') == '/Applications'
assert os.statvfs(root).f_flag & os.ST_RDONLY, 'Mounted image must be read-only'
info = plistlib.loads(subprocess.check_output(['/usr/bin/hdiutil', 'imageinfo', '-plist', sys.argv[2]]))
assert info['Format'] == 'UDZO', 'Expected a compressed read-only UDZO image'
with DSStore.open(str(root / '.DS_Store'), 'r') as metadata:
    window = metadata['.']['bwsp']
    for key in ['ShowStatusBar', 'ShowTabView', 'ShowToolbar', 'ShowPathbar', 'ShowSidebar']:
        assert window[key] is False, f'{key} must be hidden'
    assert metadata['Never Sleep.app']['Iloc'] == (190, 220)
    assert metadata['Applications']['Iloc'] == (470, 220)
    assert metadata['.']['icvp']['backgroundType'] == 2
print('Verified: read-only image, two Finder items, Applications link, background, and icon layout.')
PY
/usr/bin/hdiutil detach "$stage/verify"
output="$project_dir/dist/Never Sleep.dmg"
/bin/mv -f "$stage/Never Sleep.dmg" "$output"
echo "Built: $output"
echo "App signature preserved. Disk-image signing and notarization are separate release steps."
