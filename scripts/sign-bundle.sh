#!/bin/bash
# Explicit inside-out signing for Sparkle. Never use --deep to sign.
set -euo pipefail
bundle="$1"
identity="$2"
args=(--force --sign "$identity")
if [[ "$identity" != '-' ]]; then args+=(--options runtime --timestamp); fi
framework="$bundle/Contents/Frameworks/Sparkle.framework"
if [[ -d "$framework" ]]; then
  base="$framework/Versions/B"
  for item in "$base/XPCServices/Downloader.xpc" "$base/XPCServices/Installer.xpc" "$base/Autoupdate" "$base/Updater.app" "$framework"; do
    /usr/bin/codesign "${args[@]}" "$item"
  done
fi
/usr/bin/codesign "${args[@]}" "$bundle"
/usr/bin/codesign --verify --deep --strict "$bundle"
