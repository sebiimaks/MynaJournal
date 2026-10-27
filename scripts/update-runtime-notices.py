#!/usr/bin/env python3
"""Refresh runtime notices from reviewed, locally installed Rust/Zig sources.

Review the resulting licenses/runtime diff before regenerating the notice bundle.
This script does not download or install anything.
"""

import argparse
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re


class NoticeText(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []

    def handle_data(self, data):
        self.parts.append(data)

    def handle_endtag(self, tag):
        if tag in {"p", "div", "h1", "h2", "h3", "li", "pre", "summary"}:
            self.parts.append("\n")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-version", required=True)
    parser.add_argument("--rust-docs", required=True, type=Path)
    parser.add_argument("--rust-source", required=True, type=Path)
    parser.add_argument("--zig-version", required=True)
    parser.add_argument("--zig", required=True, type=Path)
    args = parser.parse_args()
    output = Path(__file__).resolve().parents[1] / "licenses/runtime"
    output.mkdir(parents=True, exist_ok=True)
    components = []

    def component(name, version):
        item = {"name": name, "version": version, "files": []}
        components.append(item)
        return item

    def save(item, name, data, source_url, source_data=None):
        (output / name).write_bytes(data)
        entry = {"path": name, "sha256": digest(data), "source_url": source_url}
        if source_data is not None:
            entry["original_sha256"] = digest(source_data)
        item["files"].append(entry)

    rust = component("Rust standard library (all-platform notice inventory)", args.rust_version)
    raw_html = (args.rust_docs / "COPYRIGHT-library.html").read_bytes()
    extractor = NoticeText()
    extractor.feed(raw_html.decode("utf-8"))
    plain = re.sub(r"\n[ \t]*\n(?:[ \t]*\n)+", "\n\n", "".join(extractor.parts))
    save(rust, "rust-library.txt", (plain.strip() + "\n").encode(),
         f"https://doc.rust-lang.org/{args.rust_version}/COPYRIGHT-library.html", raw_html)
    # The report identifies in-tree licenses by name; include their full texts too.
    for path in sorted((args.rust_docs / "licenses").glob("*.txt")):
        save(rust, "rust-" + path.name, path.read_bytes(),
             f"https://doc.rust-lang.org/{args.rust_version}/licenses/{path.name}")
    for relative, name in [
        ("library/compiler-builtins/LICENSE.txt", "rust-compiler-builtins.txt"),
        ("library/compiler-builtins/libm/LICENSE.txt", "rust-libm.txt"),
        ("src/llvm-project/libunwind/LICENSE.TXT", "rust-libunwind.txt"),
    ]:
        path = args.rust_source / relative
        save(rust, name, path.read_bytes(),
             f"https://github.com/rust-lang/rust/tree/{args.rust_version}/{relative}")

    zig = component("Zig runtime and libunwind (static Linux test builds)", args.zig_version)
    for relative, name in [("LICENSE", "zig.txt"), ("lib/libunwind/LICENSE.TXT", "zig-libunwind.txt")]:
        save(zig, name, (args.zig / relative).read_bytes(),
             f"https://github.com/ziglang/zig/blob/{args.zig_version}/{relative}")

    musl = component("musl (as bundled with Zig; static Linux test builds)", args.zig_version)
    musl_root = args.zig / "lib/libc/musl"
    base_url = f"https://github.com/ziglang/zig/tree/{args.zig_version}/lib/libc/musl"
    save(musl, "musl.txt", (musl_root / "COPYRIGHT").read_bytes(), base_url + "/COPYRIGHT")
    # musl's COPYRIGHT points to additional notices in individual source files.
    # Preserve those comments, including the mathematical routines' BSD grants.
    comments = {}
    for path in sorted(musl_root.rglob("*")):
        if not path.is_file() or path.suffix not in {".c", ".h", ".s", ".S"}:
            continue
        source = path.read_text(encoding="utf-8")
        for comment in re.findall(r"/\*.*?\*/", source, re.DOTALL):
            if re.search(r"copyright|permission|redistribution|public domain|SPDX", comment, re.I):
                comments.setdefault(comment, []).append(path.relative_to(musl_root).as_posix())
    text = "Additional musl source notices (unmodified comment text)\n\n"
    for comment, paths in comments.items():
        text += "Files: " + ", ".join(paths) + "\n" + comment + "\n\n"
    save(musl, "musl-source-notices.txt", text.encode(), base_url)
    # Some ports identify licences outside the installed toolchain (e.g. Go math).
    # Keep those reviewed texts and provenance when refreshing local runtime files.
    supplemental = output.parent / "runtime-supplemental.json"
    components.extend(json.loads(supplemental.read_text())["components"])
    (output / "manifest.json").write_text(json.dumps({"components": components}, indent=2) + "\n")


if __name__ == "__main__":
    main()
