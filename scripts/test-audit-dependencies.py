#!/usr/bin/env python3
import copy
from datetime import date
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("audit_dependencies", Path(__file__).with_name("audit-dependencies.py"))
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class AdvisoryPolicyTests(unittest.TestCase):
    def setUp(self):
        self.today = date(2026, 10, 28)
        self.finding = {"advisory": {"id": "RUSTSEC-EXAMPLE"}, "package": {"name": "example", "version": "1.0.0"}}
        self.report = {"database": {"advisory-count": 1}, "vulnerabilities": {"count": 1, "list": [self.finding]}, "warnings": {}}
        self.policy = {"schema": 1, "exceptions": [{"id": "RUSTSEC-EXAMPLE", "package": "example", "version": "1.0.0", "category": "vulnerability", "expires": "2026-12-31", "reason": "Reviewed fixture", "must_be_unused": True}]}

    def check(self):
        return AUDIT.check_report(self.report, self.policy, set(), self.today)

    def test_exact_current_exception_is_accepted(self):
        self.assertEqual(len(self.check()), 1)

    def test_new_advisory_or_changed_version_is_rejected(self):
        for field, value in (("id", "RUSTSEC-NEW"), ("version", "1.0.1")):
            with self.subTest(field=field):
                report = copy.deepcopy(self.report)
                report["vulnerabilities"]["list"][0]["advisory" if field == "id" else "package"][field] = value
                with self.assertRaises(ValueError):
                    AUDIT.check_report(report, self.policy, set(), self.today)

    def test_expired_exception_is_rejected_on_expiry_date(self):
        self.policy["exceptions"][0]["expires"] = self.today.isoformat()
        with self.assertRaisesRegex(ValueError, "expired"):
            self.check()

    def test_enabling_previously_unused_package_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "now enabled"):
            AUDIT.check_report(self.report, self.policy, {("example", "1.0.0")}, self.today)

    def test_new_unsoundness_warning_is_not_silently_ignored(self):
        self.report["warnings"]["unsound"] = [self.finding]
        with self.assertRaisesRegex(ValueError, "unsound"):
            self.check()

    def test_missing_database_and_partial_reports_fail_closed(self):
        self.report["database"]["advisory-count"] = 0
        with self.assertRaises(ValueError):
            self.check()
        self.report["database"]["advisory-count"] = 1
        self.report["vulnerabilities"]["count"] = 2
        with self.assertRaises(ValueError):
            self.check()

    def test_duplicate_and_obsolete_exceptions_are_rejected(self):
        self.policy["exceptions"].append(copy.deepcopy(self.policy["exceptions"][0]))
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            self.check()
        self.policy["exceptions"].pop()
        self.report["vulnerabilities"] = {"count": 0, "list": []}
        with self.assertRaisesRegex(ValueError, "obsolete"):
            self.check()


if __name__ == "__main__":
    unittest.main()
