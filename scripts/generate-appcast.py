#!/usr/bin/env python3
"""Generate or fetch a Sparkle feed. Only public release metadata is handled here."""
import argparse
import base64
import datetime
import json
import os
from pathlib import Path
import plistlib
import re
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET

REPOSITORY = "nearbycoder/never-sleep"
SITE = "https://nearbycoder.github.io/never-sleep/"
SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", SPARKLE)


def version(value):
    if not re.fullmatch(r"\d+\.\d+\.\d+", value):
        raise ValueError("Expected a stable three-part version")
    return value


def signature(value):
    if len(base64.b64decode(value, validate=True)) != 64:
        raise ValueError("Expected a 64-byte Ed25519 signature")
    return value


def feed():
    root = ET.Element("rss", version="2.0")
    channel = ET.SubElement(root, "channel")
    ET.SubElement(channel, "title").text = "Never Sleep Updates"
    ET.SubElement(channel, "link").text = SITE
    ET.SubElement(channel, "description").text = "Signed updates for Never Sleep on macOS."
    ET.SubElement(channel, "language").text = "en"
    return root, channel


def generate(plist_path, archive_path, ed_signature):
    with Path(plist_path).open("rb") as stream:
        info = plistlib.load(stream)
    release = version(info["CFBundleShortVersionString"])
    build = info["CFBundleVersion"]
    if not re.fullmatch(r"[1-9]\d*", build):
        raise ValueError("CFBundleVersion must be a positive, increasing integer")
    if info["CFBundleIdentifier"] != "com.nearby.never-sleep":
        raise ValueError("Unexpected app identifier")
    minimum = info["LSMinimumSystemVersion"]
    if not re.fullmatch(r"\d+(?:\.\d+){0,2}", minimum):
        raise ValueError("Invalid minimum system version")
    minimum = ".".join((minimum.split(".") + ["0", "0"])[:3])
    size = Path(archive_path).stat().st_size
    if size <= 0:
        raise ValueError("Empty release archive")
    root, channel = feed()
    item = ET.SubElement(channel, "item")
    ET.SubElement(item, "title").text = f"Never Sleep {release}"
    ET.SubElement(item, "link").text = f"https://github.com/{REPOSITORY}/releases/tag/v{release}"
    ET.SubElement(item, f"{{{SPARKLE}}}version").text = build
    ET.SubElement(item, f"{{{SPARKLE}}}shortVersionString").text = release
    ET.SubElement(item, f"{{{SPARKLE}}}minimumSystemVersion").text = minimum
    ET.SubElement(item, f"{{{SPARKLE}}}hardwareRequirements").text = "arm64"
    ET.SubElement(item, "pubDate").text = datetime.datetime.now(datetime.timezone.utc).strftime("%a, %d %b %Y %H:%M:%S %z")
    ET.SubElement(item, "description").text = f"Never Sleep {release} is ready. See the GitHub release for changes."
    ET.SubElement(item, "enclosure", {
        "url": f"https://github.com/{REPOSITORY}/releases/download/v{release}/Never-Sleep.dmg",
        f"{{{SPARKLE}}}edSignature": signature(ed_signature),
        "length": str(size), "type": "application/octet-stream",
    })
    ET.indent(root)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True) + b"\n"


def validate(data, tag, archive_size):
    """Fail closed if published metadata points to a different archive or release."""
    release = version(tag.removeprefix("v"))
    if tag != f"v{release}":
        raise ValueError("Unexpected release tag")
    if len(data) > 1_000_000 or b"<!DOCTYPE" in data.upper() or b"<!ENTITY" in data.upper():
        raise ValueError("Invalid feed content")
    root = ET.fromstring(data)
    items = root.findall("channel/item")
    if root.tag != "rss" or len(items) != 1:
        raise ValueError("Expected one release in feed")
    item = items[0]
    if item.findtext(f"{{{SPARKLE}}}shortVersionString") != release:
        raise ValueError("Feed version does not match release")
    if not re.fullmatch(r"[1-9]\d*", item.findtext(f"{{{SPARKLE}}}version", "")):
        raise ValueError("Invalid build number")
    enclosure = item.find("enclosure")
    if enclosure is None:
        raise ValueError("Missing update archive")
    expected = f"https://github.com/{REPOSITORY}/releases/download/{tag}/Never-Sleep.dmg"
    if enclosure.get("url") != expected:
        raise ValueError("Unexpected update download URL")
    if int(enclosure.get("length", "0")) != archive_size or archive_size <= 0:
        raise ValueError("Archive length does not match release asset")
    signature(enclosure.get(f"{{{SPARKLE}}}edSignature", ""))
    return release


def fetch_latest(destination):
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(f"https://api.github.com/repos/{REPOSITORY}/releases/latest")
    request.add_header("Accept", "application/vnd.github+json")
    if os.environ.get("GH_TOKEN"):
        request.add_header("Authorization", "Bearer " + os.environ["GH_TOKEN"])
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            release = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        # A new repository may publish its website before its first release.
        root, _ = feed()
        (destination / "appcast.xml").write_bytes(ET.tostring(root, encoding="utf-8", xml_declaration=True))
        (destination / "release.json").write_text('{"available": false}\n')
        print("No stable release yet; published an empty feed.")
        return
    assets = {asset["name"]: asset for asset in release["assets"]}
    asset = assets["appcast.xml"]
    expected = f"https://github.com/{REPOSITORY}/releases/download/{release['tag_name']}/appcast.xml"
    if asset["browser_download_url"] != expected or asset["size"] > 1_000_000:
        raise ValueError("Unexpected feed asset")
    with urllib.request.urlopen(expected, timeout=30) as response:
        data = response.read(1_000_001)
    current = validate(data, release["tag_name"], assets["Never-Sleep.dmg"]["size"])
    # Keep exact signed bytes. Re-serializing XML would invalidate its signature.
    (destination / "appcast.xml").write_bytes(data)
    (destination / "release.json").write_text(json.dumps({
        "available": True, "version": current, "tag": release["tag_name"],
        "url": release["html_url"], "published_at": release["published_at"],
        "download": assets["Never-Sleep.dmg"]["browser_download_url"],
    }, indent=2) + "\n")
    print(f"Prepared update feed for {release['tag_name']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("generate")
    for name in ("plist", "archive", "signature", "output"):
        build.add_argument("--" + name, required=True)
    fetch = commands.add_parser("fetch-latest")
    fetch.add_argument("--directory", required=True)
    args = parser.parse_args()
    if args.command == "generate":
        Path(args.output).write_bytes(generate(args.plist, args.archive, args.signature))
    else:
        fetch_latest(args.directory)


if __name__ == "__main__":
    main()
