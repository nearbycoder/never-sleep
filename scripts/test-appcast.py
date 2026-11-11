#!/usr/bin/env python3
"""Exercise release metadata boundaries without credentials or network access."""
import base64
import importlib.util
from pathlib import Path
import plistlib
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("appcast", Path(__file__).with_name("generate-appcast.py"))
appcast = importlib.util.module_from_spec(spec)
spec.loader.exec_module(appcast)


class AppcastTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.plist = self.root / "Info.plist"
        self.archive = self.root / "Never-Sleep.dmg"
        self.archive.write_bytes(b"release-archive")
        self.size = self.archive.stat().st_size
        self.signature = base64.b64encode(bytes(64)).decode()
        with self.plist.open("wb") as stream:
            plistlib.dump({"CFBundleIdentifier": "com.nearby.never-sleep", "CFBundleVersion": "2",
                          "CFBundleShortVersionString": "0.2.0", "LSMinimumSystemVersion": "14.0"}, stream)
        self.data = appcast.generate(self.plist, self.archive, self.signature)

    def test_release_targets_exact_version_and_platform(self):
        self.assertEqual(appcast.validate(self.data, "v0.2.0", self.size), "0.2.0")
        item = ET.fromstring(self.data).find("channel/item")
        self.assertEqual(item.findtext(f"{{{appcast.SPARKLE}}}minimumSystemVersion"), "14.0.0")
        self.assertEqual(item.findtext(f"{{{appcast.SPARKLE}}}hardwareRequirements"), "arm64")

    def test_rejects_changed_archive_length(self):
        with self.assertRaises(ValueError):
            appcast.validate(self.data, "v0.2.0", self.size + 1)

    def test_rejects_foreign_archive_url(self):
        with self.assertRaises(ValueError):
            appcast.validate(self.data.replace(b"github.com/nearbycoder", b"evil.example/nearbycoder"), "v0.2.0", self.size)

    def test_rejects_mismatched_release(self):
        with self.assertRaises(ValueError):
            appcast.validate(self.data, "v0.3.0", self.size)

    def test_rejects_missing_or_malformed_signature(self):
        for value in ("", "not-base64", base64.b64encode(bytes(32)).decode()):
            with self.subTest(value=value), self.assertRaises(ValueError):
                appcast.generate(self.plist, self.archive, value)

    def test_rejects_entity_declarations(self):
        with self.assertRaises(ValueError):
            appcast.validate(b'<!DOCTYPE rss [<!ENTITY example "test">]>' + self.data, "v0.2.0", self.size)

    def test_rejects_empty_archive(self):
        self.archive.write_bytes(b"")
        with self.assertRaises(ValueError):
            appcast.generate(self.plist, self.archive, self.signature)


if __name__ == "__main__":
    unittest.main()
