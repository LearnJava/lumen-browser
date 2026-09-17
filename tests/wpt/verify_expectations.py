import itertools
import json
import unittest

import expectations
from mozlog import structuredlog
from wptrunner.formatters.wptreport import WptreportFormatter


TEST_ID = "/expectations-probe.html"


def reported_result(expected, actual, subtest):
    formatter = WptreportFormatter()
    logger = structuredlog.StructuredLogger("verify-wpt-expectations")
    handler = formatter
    logger.add_handler(handler)
    try:
        logger.suite_start([TEST_ID])
        logger.test_start(TEST_ID)
        kwargs = {"expected": expected[0], "known_intermittent": list(expected[1:])}
        if subtest:
            logger.test_status(TEST_ID, "probe", actual, **kwargs)
            logger.test_end(TEST_ID, "OK")
        else:
            logger.test_end(TEST_ID, actual, **kwargs)
        logger.suite_end()
    finally:
        logger.remove_handler(handler)
    return json.loads(json.dumps(formatter.results))["results"]


class ExpectationsTests(unittest.TestCase):
    def test_allowed_statuses_ignore_order(self):
        for subtest, good, bad in ((True, "PASS", "FAIL"), (False, "OK", "ERROR")):
            for statuses in ((good, bad), (good, "TIMEOUT"), (bad, "TIMEOUT")):
                for expected in itertools.permutations(statuses):
                    for actual in statuses:
                        with self.subTest(subtest=subtest, expected=expected, actual=actual):
                            result = expectations.classify(
                                reported_result(expected, actual, subtest), [TEST_ID]
                            )
                            self.assertEqual(result, {
                                "regressions": [], "improvements": [], "other": []
                            })

    def test_unexpected_statuses_ignore_order(self):
        for subtest, good, bad in ((True, "PASS", "FAIL"), (False, "OK", "ERROR")):
            for actual, reason in (("TIMEOUT", "new TIMEOUT"),
                                   ("NOTRUN" if subtest else "CRASH", "expected PASS regressed")):
                verdicts = []
                for expected in itertools.permutations((good, bad)):
                    with self.subTest(subtest=subtest, expected=expected, actual=actual):
                        verdict = expectations.classify(
                            reported_result(expected, actual, subtest), [TEST_ID]
                        )
                        self.assertEqual(len(verdict["regressions"]), 1)
                        self.assertEqual(verdict["regressions"][0]["reason"], reason)
                        self.assertEqual(verdict["improvements"], [])
                        self.assertEqual(verdict["other"], [])
                        verdicts.append(verdict)
                self.assertEqual(verdicts[0], verdicts[1])

    def test_scalar_expectations_preserve_classification(self):
        cases = (
            (True, "PASS", "FAIL", "regressions", "expected PASS regressed"),
            (False, "OK", "ERROR", "regressions", "expected PASS regressed"),
            (False, "PASS", "FAIL", "regressions", "expected PASS regressed"),
            (True, "FAIL", "TIMEOUT", "regressions", "new TIMEOUT"),
            (False, "ERROR", "TIMEOUT", "regressions", "new TIMEOUT"),
            (True, "FAIL", "PASS", "improvements", "unexpected PASS — narrow expectations"),
            (False, "ERROR", "OK", "improvements", "unexpected PASS — narrow expectations"),
            (True, "FAIL", "NOTRUN", "other", "status changed"),
            (False, "ERROR", "CRASH", "other", "status changed"),
        )
        for subtest, expected, actual, bucket, reason in cases:
            with self.subTest(subtest=subtest, expected=expected, actual=actual):
                verdict = expectations.classify(
                    reported_result((expected,), actual, subtest), [TEST_ID]
                )
                self.assertEqual(sum(len(entries) for entries in verdict.values()), 1)
                self.assertEqual(verdict[bucket], [{
                    "test": TEST_ID,
                    "subtest": "probe" if subtest else None,
                    "expected": expected,
                    "actual": actual,
                    "reason": reason,
                }])

    def test_unexpected_pass_with_only_bad_expectations(self):
        for expected in itertools.permutations(("FAIL", "TIMEOUT")):
            verdict = expectations.classify(reported_result(expected, "PASS", True), [TEST_ID])
            self.assertEqual(len(verdict["improvements"]), 1)
            self.assertEqual(verdict["regressions"], [])

    def test_default_results_and_missing_ids(self):
        results = [{"test": TEST_ID + "?variant", "status": "OK", "subtests": [
            {"name": "probe", "status": "PASS"}
        ]}]
        verdict = expectations.classify(results, [TEST_ID, "/missing.html"])
        self.assertEqual(len(verdict["regressions"]), 1)
        self.assertEqual(verdict["regressions"][0]["test"], "/missing.html")
        self.assertEqual(verdict["regressions"][0]["actual"], "MISSING")

    def test_unexpressible_subtest_still_skipped(self):
        results = reported_result(("PASS",), "FAIL", True)
        results[0]["subtests"][0]["name"] = "line\nbreak"
        self.assertEqual(expectations.classify(results, [TEST_ID])["regressions"], [])


if __name__ == "__main__":
    unittest.main()
