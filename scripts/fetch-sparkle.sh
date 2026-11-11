#!/bin/bash
# Verified upstream framework and release tools, cached outside source control.
set -euo pipefail
cd "$(dirname "$0")/.."
version=2.10.1
checksum=1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27
mkdir -p target/sparkle
archive="target/sparkle/Sparkle-$version.tar.xz"
if [[ ! -f "$archive" ]]; then
  curl --fail --location --retry 3 "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz" -o "$archive"
fi
printf '%s  %s\n' "$checksum" "$archive" | shasum -a 256 --check
# Always extract the verified archive, not potentially modified cached binaries.
tar -xJf "$archive" -C target/sparkle
