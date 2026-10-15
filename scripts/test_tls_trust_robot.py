#!/usr/bin/env python3
"""Тесты робота доверия T (COMPAT-CORPUS-5): дифф снимков, база по дням, список хостов."""

import io
import os
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tls_trust_robot as r  # noqa: E402


def row(host, **kw):
    base = {
        "host": host, "port": 443, "handshake_ok": True, "seen_chain": True, "error": "",
        "trusted": True, "trust_error": "", "tls_version": "TLS 1.3",
        "issuer_cn": "R11", "issuer_org": "Let's Encrypt", "root_kind": "webpki",
        "anchor_name": "ISRG Root X1", "sct_logs": 2, "revocation": "no_staple",
        "leaf_sha256": "AA",
    }
    base.update(kw)
    return base


def kinds(events):
    return [(e["kind"], e["host"]) for e in events]


class Diff(unittest.TestCase):
    def test_no_change_no_events(self):
        a = {"a.ru": row("a.ru")}
        self.assertEqual(r.diff_snapshots(a, dict(a)), [])

    def test_leaf_renewal_is_not_an_event(self):
        self.assertEqual(r.diff_snapshots({"a.ru": row("a.ru")},
                                          {"a.ru": row("a.ru", leaf_sha256="BB")}), [])

    def test_substituted_issuer_is_caught(self):
        old = {"bank.ru": row("bank.ru")}
        new = {"bank.ru": row("bank.ru", issuer_cn="Russian Trusted Sub CA",
                              issuer_org="The Ministry of Digital Development", root_kind="nuc",
                              anchor_name="Russian Trusted Root CA", sct_logs=0)}
        got = kinds(r.diff_snapshots(old, new))
        self.assertIn(("root_kind_changed", "bank.ru"), got)
        self.assertIn(("ca_changed", "bank.ru"), got)
        self.assertIn(("sct_dropped", "bank.ru"), got)
        ev = [e for e in r.diff_snapshots(old, new) if e["kind"] == "root_kind_changed"][0]
        self.assertIn("НУЦ", ev["detail"])

    def test_lost_trust_and_revocation(self):
        old = {"a.ru": row("a.ru")}
        new = {"a.ru": row("a.ru", trusted=False, trust_error="expired", revocation="revoked")}
        got = kinds(r.diff_snapshots(old, new))
        self.assertEqual(got[:2], [("lost_trust", "a.ru"), ("revoked", "a.ru")])

    def test_handshake_failure_and_recovery(self):
        down = row("a.ru", handshake_ok=False, seen_chain=False, error="timeout")
        self.assertEqual(kinds(r.diff_snapshots({"a.ru": row("a.ru")}, {"a.ru": down})),
                         [("handshake_failed", "a.ru")])
        self.assertEqual(kinds(r.diff_snapshots({"a.ru": down}, {"a.ru": row("a.ru")})),
                         [("handshake_recovered", "a.ru")])

    def test_sct_dip_above_minimum_is_ignored(self):
        old = {"a.ru": row("a.ru", sct_logs=4)}
        self.assertEqual(r.diff_snapshots(old, {"a.ru": row("a.ru", sct_logs=3)}), [])

    def test_hosts_added_and_removed(self):
        got = kinds(r.diff_snapshots({"a.ru": row("a.ru")}, {"b.ru": row("b.ru")}))
        self.assertEqual(sorted(got), [("host_added", "b.ru"), ("host_removed", "a.ru")])

    def test_events_ordered_most_important_first(self):
        old = {"a.ru": row("a.ru"), "b.ru": row("b.ru")}
        new = {"a.ru": row("a.ru", issuer_org="Other CA"),
               "b.ru": row("b.ru", trusted=False, trust_error="x")}
        self.assertEqual([e["kind"] for e in r.diff_snapshots(old, new)],
                         ["lost_trust", "ca_changed"])


class Database(unittest.TestCase):
    def test_snapshot_roundtrip_sorted_by_host(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "2026-10-14.jsonl")
            r.write_snapshot(p, [row("b.ru"), row("a.ru", error="Привет")])
            self.assertEqual(list(r.read_snapshot(p)), ["a.ru", "b.ru"])
            self.assertEqual(r.read_snapshot(p)["a.ru"]["error"], "Привет")

    def test_previous_snapshot_is_the_latest_older_day(self):
        with tempfile.TemporaryDirectory() as d:
            for n in ("2026-10-12.jsonl", "2026-10-13.jsonl", "2026-10-14.jsonl", "2026-10-13.md"):
                open(os.path.join(d, n), "w").close()
            self.assertEqual(r.previous_snapshot("2026-10-14", d), "2026-10-13.jsonl")
            self.assertIsNone(r.previous_snapshot("2026-10-12", d))

    def test_two_runs_second_day_reports_the_change(self):
        day1 = [row("bank.ru"), row("gov.ru")]
        day2 = [row("bank.ru", issuer_org="Foreign CA"), row("gov.ru")]
        with tempfile.TemporaryDirectory() as d:
            for date, rows in (("2026-10-13", day1), ("2026-10-14", day2)):
                args = r.argparse.Namespace(lumen="fake", list="x", db=d, date=date, point="ru",
                                            timeout=1, fail_on_change=True)
                with mock.patch.object(r, "run_probe", return_value=(rows, 3.0)), \
                        redirect_stdout(io.StringIO()):
                    code = r.cmd_run(args)
                self.assertEqual(code, 0 if date == "2026-10-13" else 2)
            report = open(os.path.join(d, "2026-10-14.md"), encoding="utf-8").read()
            self.assertIn("`ca_changed` bank.ru", report)
            self.assertIn("Дифф с 2026-10-13.jsonl", report)
            base = open(os.path.join(d, "2026-10-13.md"), encoding="utf-8").read()
            self.assertIn("базовая линия", base)


class Summary(unittest.TestCase):
    def test_counts(self):
        rows = {"a.ru": row("a.ru", revocation="good"),
                "b.ru": row("b.ru", root_kind="nuc", sct_logs=0, trusted=False),
                "c.ru": row("c.ru", handshake_ok=False, seen_chain=False)}
        s = r.summarize(rows)
        self.assertEqual((s["hosts"], s["handshake_ok"], s["trusted"]), (3, 2, 1))
        self.assertEqual(s["failed"], ["c.ru"])
        self.assertEqual(s["untrusted"], ["b.ru"])
        self.assertEqual(s["few_sct"], ["b.ru"])
        self.assertEqual(s["root_kind"], {"webpki": 1, "nuc": 1})
        self.assertEqual(s["staple"], 1)


if __name__ == "__main__":
    unittest.main()
