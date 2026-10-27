#!/usr/bin/env python3
"""Regression checks for the distribution-notice integrity gate."""

import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("generate_notices", ROOT / "scripts/generate-notices.py")
NOTICES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NOTICES)


class NoticeIntegrityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        shutil.copyfile(ROOT / "Cargo.lock", self.root / "Cargo.lock")
        shutil.copytree(ROOT / "licenses", self.root / "licenses")

    def test_current_snapshot_reproduces_distributed_notice(self):
        self.assertEqual(NOTICES.generate(self.root), (ROOT / "THIRD_PARTY_NOTICES.txt").read_bytes())

    def test_new_locked_dependency_requires_review(self):
        with (self.root / "Cargo.lock").open("a") as handle:
            handle.write('\n[[package]]\nname = "unreviewed-example"\nversion = "1.0.0"\n'
                         'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                         'checksum = "unreviewed"\n')
        with self.assertRaisesRegex(ValueError, "Cargo.lock differs"):
            NOTICES.generate(self.root)

    def test_changed_crate_checksum_requires_review(self):
        path = self.root / "Cargo.lock"
        text = path.read_text()
        path.write_text(text.replace('checksum = "', 'checksum = "changed-', 1))
        with self.assertRaisesRegex(ValueError, "Cargo.lock differs"):
            NOTICES.generate(self.root)

    def test_altered_copyright_text_is_rejected(self):
        path = self.root / "licenses/texts.json"
        texts = json.loads(path.read_text())
        texts[next(iter(texts))] = "A notice was accidentally lost."
        path.write_text(json.dumps(texts))
        with self.assertRaisesRegex(ValueError, "Notice text hash mismatch"):
            NOTICES.generate(self.root)

    def test_missing_mpl_source_text_is_rejected(self):
        snapshot = json.loads((self.root / "licenses/registry.json").read_text())
        package = next(p for p in snapshot["packages"] if p["name"] == "option-ext")
        record = next(r for r in package["source_files"] if r["label"] == "src/lib.rs")
        path = self.root / "licenses/texts.json"
        texts = json.loads(path.read_text())
        del texts[record["sha256"]]
        path.write_text(json.dumps(texts))
        with self.assertRaisesRegex(ValueError, "Missing MPL source text"):
            NOTICES.generate(self.root)

    def test_partially_removed_mpl_source_inventory_is_rejected(self):
        path = self.root / "licenses/registry.json"
        snapshot = json.loads(path.read_text())
        package = next(p for p in snapshot["packages"] if p["name"] == "option-ext")
        package["source_files"].pop()
        path.write_text(json.dumps(snapshot))
        with self.assertRaisesRegex(ValueError, "Incomplete MPL source file inventory"):
            NOTICES.generate(self.root)

    def test_altered_native_runtime_notice_is_rejected(self):
        runtime = self.root / "licenses/runtime"
        manifest = json.loads((runtime / "manifest.json").read_text())
        path = runtime / manifest["components"][0]["files"][0]["path"]
        path.write_text("A native runtime notice was accidentally lost.")
        with self.assertRaisesRegex(ValueError, "Runtime notice hash mismatch"):
            NOTICES.generate(self.root)


if __name__ == "__main__":
    unittest.main()
