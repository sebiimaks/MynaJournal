#!/usr/bin/env python3
"""Check RustSec findings against narrow, expiring review decisions."""

import argparse
from datetime import date, datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def check_report(report, policy, active_packages, today):
    if policy.get("schema") != 1:
        raise ValueError("Unsupported advisory-exception schema")
    if report.get("database", {}).get("advisory-count", 0) <= 0:
        raise ValueError("Audit did not use a populated advisory database")
    vulnerabilities = report["vulnerabilities"]["list"]
    if report["vulnerabilities"]["count"] != len(vulnerabilities):
        raise ValueError("Incomplete vulnerability report")
    findings = [("vulnerability", item) for item in vulnerabilities]
    for category, entries in report["warnings"].items():
        findings.extend((category, item) for item in entries)
    exceptions = {}
    for item in policy["exceptions"]:
        key = (item["category"], item["id"], item["package"], item["version"])
        if key in exceptions:
            raise ValueError(f"Duplicate exception: {key}")
        if not item["reason"].strip() or date.fromisoformat(item["expires"]) <= today:
            raise ValueError(f"Missing explanation or expired exception: {item['id']}")
        if item.get("must_be_unused") and (item["package"], item["version"]) in active_packages:
            raise ValueError(f"Previously unused vulnerable package is now enabled: {item['package']}")
        exceptions[key] = item
    accepted = []
    failures = []
    for category, item in findings:
        advisory = item.get("advisory") or {}
        package = item["package"]
        key = (category, advisory.get("id"), package["name"], package["version"])
        exception = exceptions.pop(key, None)
        if exception is None:
            failures.append(f"{category}: {key[1] or 'unidentified warning'} — {key[2]} {key[3]}")
        else:
            accepted.append(f"Reviewed until {exception['expires']}: {key[1]} — {key[2]} {key[3]}")
    if exceptions:
        failures.extend(f"Remove obsolete exception: {key[1]} — {key[2]} {key[3]}" for key in exceptions)
    if failures:
        raise ValueError("\n".join(failures))
    return accepted


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audit-bin", default="cargo-audit")
    parser.add_argument("--database", type=Path)
    parser.add_argument("--no-fetch", action="store_true")
    args = parser.parse_args()
    command = [args.audit_bin, "audit", "--json", "--no-yanked"]
    if args.database:
        command.extend(["--db", str(args.database)])
    if args.no_fetch:
        command.append("--no-fetch")
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
    if result.returncode not in (0, 1) or not result.stdout.strip():
        raise ValueError("Dependency audit failed: " + result.stderr.strip())
    report = json.loads(result.stdout)
    tree = subprocess.check_output([
        "cargo", "tree", "--locked", "--offline", "--all-features", "--target", "all",
        "--edges", "normal,build", "--prefix", "none", "--format", "{p}",
    ], cwd=ROOT, text=True)
    active = set(re.findall(r"^([\w-]+) v([^\s]+)", tree, re.MULTILINE))
    if not active:
        raise ValueError("Could not determine the active dependency graph")
    policy = json.loads((ROOT / "security/advisory-exceptions.json").read_text())
    for decision in check_report(report, policy, active, datetime.now(timezone.utc).date()):
        print(decision)
    print("No unreviewed RustSec findings. Exceptions match exact versions and have not expired.")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, TypeError, OSError, subprocess.CalledProcessError) as error:
        print(f"Dependency security check failed: {error}", file=sys.stderr)
        sys.exit(1)
