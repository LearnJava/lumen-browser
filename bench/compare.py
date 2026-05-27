#!/usr/bin/env python3
"""
Benchmark regression gate: compare current results against baseline.json.

Runs `cargo run -p lumen-bench --release`, parses output, and compares
against baseline.json. Fails (exit 1) if median or p95 regress > 5%.

Usage:
    python bench/compare.py
    python bench/compare.py --baseline bench/baseline.json  # explicit baseline
"""

import sys
import json
import subprocess
import re
import os
from pathlib import Path
from typing import Dict, Tuple
from datetime import datetime

# Ensure UTF-8 output on Windows
if sys.stdout.encoding != 'utf-8':
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

REGRESSION_THRESHOLD = 0.05  # 5%

def get_baseline(baseline_path: str) -> Dict:
    """Load baseline metrics from JSON file."""
    path = Path(baseline_path)
    if not path.exists():
        raise FileNotFoundError(f"Baseline not found: {baseline_path}")
    with open(path) as f:
        return json.load(f)

def run_bench() -> str:
    """Run cargo bench and capture output."""
    env = os.environ.copy()
    env["PATH"] = f"/c/Users/konstantin/.cargo/bin:{env.get('PATH', '')}"
    result = subprocess.run(
        ["cargo", "run", "-p", "lumen-bench", "--release"],
        capture_output=True,
        text=True,
        cwd=os.getcwd(),
        env=env,
    )
    if result.returncode != 0:
        print(f"Bench failed:\n{result.stderr}", file=sys.stderr)
        sys.exit(1)
    return result.stdout

def parse_bench_output(output: str) -> Dict[str, Dict[str, float]]:
    """Parse bench output like: 'decode  min 1.1 μs med 1.3 μs mean 1.5 μs p95 2.4 μs max 2.8 μs'."""
    metrics = {}
    # Match lines with phase name and 5 values (min, median, mean, p95, max)
    pattern = r'^\s*(\w+)\s+min\s+([\d.]+)\s+\w+\s+med\s+([\d.]+)\s+\w+\s+mean\s+([\d.]+)\s+\w+\s+p95\s+([\d.]+)\s+\w+\s+max\s+([\d.]+)'
    for line in output.split('\n'):
        if not line.strip():
            continue
        match = re.match(pattern, line)
        if match:
            phase, min_v, med_v, mean_v, p95_v, max_v = match.groups()
            metrics[phase] = {
                'min': float(min_v),
                'median': float(med_v),
                'mean': float(mean_v),
                'p95': float(p95_v),
                'max': float(max_v),
            }
    return metrics

def compare_metrics(baseline: Dict[str, Dict], current: Dict[str, Dict]) -> Tuple[bool, str]:
    """
    Compare current metrics against baseline.
    Returns (passed, report_text) where passed = no regressions > threshold.
    """
    report_lines = []
    regressions = []

    for phase in sorted(current.keys()):
        if phase not in baseline['metrics']:
            report_lines.append(f"  {phase}: NEW (no baseline)")
            continue

        base_metrics = baseline['metrics'][phase]
        curr_metrics = current[phase]

        # Check median and p95 (primary metrics)
        for metric_name in ['median', 'p95']:
            base_val = base_metrics.get(metric_name, 0)
            curr_val = curr_metrics.get(metric_name, 0)
            if base_val == 0:
                continue

            change = (curr_val - base_val) / base_val
            change_pct = change * 100

            # Mark regression
            symbol = 'OK'
            if change > REGRESSION_THRESHOLD:
                symbol = 'FAIL'
                regressions.append((phase, metric_name, change_pct))
            elif change < -REGRESSION_THRESHOLD:
                symbol = 'IMPR'

            base_str = f"{base_val:.1f}"
            curr_str = f"{curr_val:.1f}"
            report_lines.append(
                f"  {phase:12} {metric_name:7} [{symbol:4}] {base_str:>7} -> {curr_str:>7} ({change_pct:+.1f}%)"
            )

    passed = len(regressions) == 0
    report = "\n".join(report_lines)

    if regressions:
        report += "\n\n[REGRESSION] Detected (> 5%):\n"
        for phase, metric, pct in regressions:
            report += f"  {phase}.{metric}: +{pct:.1f}%\n"

    return passed, report

def main():
    baseline_path = "bench/baseline.json"
    for i, arg in enumerate(sys.argv[1:]):
        if arg == "--baseline" and i + 1 < len(sys.argv) - 1:
            baseline_path = sys.argv[i + 2]

    print(f"Loading baseline from {baseline_path}...")
    baseline = get_baseline(baseline_path)
    baseline_ts = baseline.get('timestamp', '?')
    print(f"  Baseline timestamp: {baseline_ts}")
    print()

    print("Running benchmark...")
    output = run_bench()
    current = parse_bench_output(output)

    if not current:
        print("ERROR: Could not parse benchmark output", file=sys.stderr)
        print(output, file=sys.stderr)
        sys.exit(1)

    print("\nComparison (baseline -> current):")
    passed, report = compare_metrics(baseline, current)
    print(report)

    if passed:
        print("\n[OK] No regressions detected")
        sys.exit(0)
    else:
        print("\n[FAIL] Regressions detected")
        sys.exit(1)

if __name__ == "__main__":
    main()
