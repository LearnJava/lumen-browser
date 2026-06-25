#!/usr/bin/env python3
"""CI baseline tracker for graphic tests.

Separates concerns:
- Debtor baselines live in `graphic_tests/run.py` (KNOWN_DEBTORS) and are checked
  by `run.py` itself. Do NOT duplicate them here.
- Non-debtor baselines live in `graphic_tests/results/baselines.json` and are
  promoted by `--update` from current PASS runs.

Usage:
    python graphic_tests/ci_baseline.py                    # check latest.json
    python graphic_tests/ci_baseline.py --json path.json    # check custom file
    python graphic_tests/ci_baseline.py --update            # promote PASS → baseline
    python graphic_tests/ci_baseline.py --tolerance 1.5     # custom noise band
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timezone

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
RESULTS_DIR = os.path.join(REPO, 'graphic_tests', 'results')
BASELINE_FILE = os.path.join(RESULTS_DIR, 'baselines.json')


def load_json(path: str) -> dict:
    with open(path, encoding='utf-8') as f:
        return json.load(f)


def save_json(path: str, data: dict) -> None:
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)


def load_non_debtor_baselines() -> dict[str, dict]:
    """Baselines promoted from historic PASS runs (non-debtor tests only)."""
    if not os.path.exists(BASELINE_FILE):
        return {}
    try:
        raw = load_json(BASELINE_FILE)
        return {k: v for k, v in raw.items() if isinstance(v, dict) and 'baseline_pct' in v}
    except Exception:
        return {}


def save_non_debtor_baselines(baselines: dict[str, dict]) -> None:
    save_json(BASELINE_FILE, baselines)


def check(
    data: dict,
    *,
    tolerance: float = 2.0,
    known_debtors: dict[str, tuple[str, float]] | None = None,
) -> tuple[list[dict], list[dict]]:
    """Return (regressions, warnings).

    - Debtor tests: delegated to run.py; here we only warn if a debtor is FAIL
      with no debtor metadata (mising KNOWN_DEBTORS entry).
    - Non-debtor FAIL: compared against baselines.json + tolerance.
    """
    regressions: list[dict] = []
    warnings: list[dict] = []
    known_debtors = known_debtors or {}

    for test in data.get('tests', []):
        tid = str(test.get('id', ''))
        status = test.get('status', '')
        diff = test.get('diff_pct')
        if diff is None:
            continue

        debtor = test.get('debtor') or {}
        debt_bug = debtor.get('bug')
        debt_base = debtor.get('baseline')

        # Debtor tests are governed by run.py; skip regression check here.
        if tid in known_debtors:
            continue

        # A FAIL with debtor metadata that is NOT in KNOWN_DEBTORS is
        # suspicious — warn, don't flag as regression.
        if debt_bug is not None and debt_base is not None:
            warnings.append({
                'id': tid,
                'diff_pct': diff,
                'reason': f'FAIL with debtor metadata but missing from KNOWN_DEBTORS (bug={debt_bug}, baseline={debt_base})',
            })
            continue

        if status != 'FAIL':
            continue

        # Non-debtor FAIL: check against historic baseline.
        baseline_entry = load_non_debtor_baselines().get(tid)
        if baseline_entry is None:
            warnings.append({
                'id': tid,
                'diff_pct': diff,
                'reason': 'FAIL without non-debtor baseline (new test or not yet promoted)',
            })
            continue

        baseline_pct = float(baseline_entry['baseline_pct'])
        if diff > baseline_pct + tolerance:
            regressions.append({
                'id': tid,
                'diff_pct': diff,
                'baseline_pct': baseline_pct,
                'reason': (
                    f'non-debtor regression: {diff:.2f}% > '
                    f'baseline {baseline_pct:.2f}% + {tolerance}% tolerance'
                ),
            })

    return regressions, warnings


def update_baseline(data: dict) -> dict:
    """Promote current PASS tests to baselines.json (non-debtors only)."""
    baselines = load_non_debtor_baselines()
    updated: dict[str, dict] = {}

    for test in data.get('tests', []):
        tid = str(test.get('id', ''))
        status = test.get('status', '')
        diff = test.get('diff_pct')
        if diff is None:
            continue

        debtor = test.get('debtor') or {}
        if debtor.get('bug') is not None:
            # Debtor baselines are managed in run.py, not here.
            continue

        if status == 'PASS':
            baselines[tid] = {
                'baseline_pct': diff,
                'promoted_at': datetime.now(timezone.utc).isoformat(),
            }
            updated[tid] = baselines[tid]

    save_non_debtor_baselines(baselines)
    return updated


def main() -> int:
    parser = argparse.ArgumentParser(description='Graphic test baseline checker')
    parser.add_argument('--json', help='Path to results JSON (default: latest.json)')
    parser.add_argument('--update', action='store_true', help='Promote PASS diffs to non-debtor baselines.json')
    parser.add_argument('--tolerance', type=float, default=2.0,
                        help='Allowed gdigrab noise band (%%), default 2.0')
    args = parser.parse_args()

    json_path = args.json or os.path.join(RESULTS_DIR, 'latest.json')
    if not os.path.exists(json_path):
        print(f'ERROR: {json_path} not found', file=sys.stderr)
        return 2

    data = load_json(json_path)

    if args.update:
        updated = update_baseline(data)
        print(json.dumps({'updated': updated}, ensure_ascii=False, indent=2))
        return 0

    # Load known debtors from run.py (source of truth).
    known_debtors: dict[str, tuple[str, float]] = {}
    run_py = os.path.join(REPO, 'graphic_tests', 'run.py')
    if os.path.exists(run_py):
        try:
            with open(run_py, encoding='utf-8') as f:
                source = f.read()
            # Extract KNOWN_DEBTORS dict literal via a safe eval of the assignment.
            start = source.find('KNOWN_DEBTORS: dict[str, tuple[str, float]] = {')
            if start != -1:
                # Find matching closing brace.
                brace_start = source.find('{', start)
                depth = 0
                end = brace_start
                for i, ch in enumerate(source[brace_start:], brace_start):
                    if ch == '{':
                        depth += 1
                    elif ch == '}':
                        depth -= 1
                        if depth == 0:
                            end = i + 1
                            break
                known_debtors = eval(source[brace_start:end])  # noqa: S307
        except Exception:
            pass

    regressions, warnings = check(data, tolerance=args.tolerance, known_debtors=known_debtors)

    report = {
        'timestamp': data.get('timestamp', ''),
        'git': data.get('git', {}),
        'summary': data.get('summary', {}),
        'known_debtors_loaded': len(known_debtors),
        'regressions': regressions,
        'warnings': warnings,
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))

    if regressions:
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
