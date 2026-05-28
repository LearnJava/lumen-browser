#!/usr/bin/env python3
"""
Benchmark regression gate: compare current results against baseline.json.

Runs `cargo run -p lumen-bench --release`, parses output, and compares
against baseline.json. Fails (exit 1) if median or p95 regress > 5% (time/RAM)
or > 20% (tier transitions).

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

# Ensure UTF-8 output on Windows
if sys.stdout.encoding != 'utf-8':
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

REGRESSION_THRESHOLD = 0.05  # 5% for time and RAM
TIER_TRANSITION_THRESHOLD = 0.20  # 20% for tier transitions

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

def parse_bench_output(output: str) -> Tuple[Dict[str, Dict[str, float]], Dict[str, float]]:
    """
    Parse bench output: time phases + RSS + PEAK_RSS + steady_state_rss.
    Returns (metrics, ram_metrics) where:
    - metrics: {phase: {min, median, mean, p95, max}}
    - ram_metrics: {metric_name: value} — includes RSS, PEAK_RSS, steady_state_rss
    """
    metrics = {}
    ram_metrics = {}

    # Match lines with phase name and 5 values (min, median, mean, p95, max)
    time_pattern = r'^\s*(\w+)\s+min\s+([\d.]+)\s+\w+\s+med\s+([\d.]+)\s+\w+\s+mean\s+([\d.]+)\s+\w+\s+p95\s+([\d.]+)\s+\w+\s+max\s+([\d.]+)'
    # Match RSS/PEAK_RSS lines with format: "RSS  min 4.30 MB  med 4.41 MB  mean 4.40 MB  p95 4.43 MB  max 4.44 MB"
    rss_pattern = r'^\s*(RSS|PEAK_RSS)\s+min\s+([\d.]+)\s+\w+\s+med\s+([\d.]+)\s+\w+\s+mean\s+([\d.]+)\s+\w+\s+p95\s+([\d.]+)\s+\w+\s+max\s+([\d.]+)'
    # Match steady_state_rss: "steady_state_rss: 4 MB"
    steady_state_pattern = r'^\s*steady_state_rss:\s+([\d.]+)\s+\w+'

    for line in output.split('\n'):
        if not line.strip():
            continue

        # Try time phase match
        match = re.match(time_pattern, line)
        if match:
            phase, min_v, med_v, mean_v, p95_v, max_v = match.groups()
            metrics[phase] = {
                'min': float(min_v),
                'median': float(med_v),
                'mean': float(mean_v),
                'p95': float(p95_v),
                'max': float(max_v),
            }
            continue

        # Try RSS/PEAK_RSS match
        match = re.match(rss_pattern, line)
        if match:
            metric_name, min_v, med_v, mean_v, p95_v, max_v = match.groups()
            metric_key = metric_name.lower()  # 'rss' or 'peak_rss'
            ram_metrics[metric_key] = {
                'min': float(min_v),
                'median': float(med_v),
                'mean': float(mean_v),
                'p95': float(p95_v),
                'max': float(max_v),
            }
            continue

        # Try steady_state_rss match
        match = re.match(steady_state_pattern, line)
        if match:
            steady_val = float(match.group(1))
            ram_metrics['steady_state_rss'] = steady_val

    return metrics, ram_metrics

def compare_metrics(baseline: Dict, current_metrics: Dict[str, Dict], current_ram: Dict[str, float]) -> Tuple[bool, str]:
    """
    Compare current metrics against baseline.
    Returns (passed, report_text) where passed = no regressions > threshold.
    Thresholds:
    - Time + RAM: 5% regression fails
    - Tier transitions (stub): 20% regression fails
    """
    report_lines = []
    regressions = []

    # Compare time metrics
    for phase in sorted(current_metrics.keys()):
        if phase not in baseline.get('metrics', {}):
            report_lines.append(f"  {phase}: NEW (no baseline)")
            continue

        base_metrics = baseline['metrics'][phase]
        curr_metrics = current_metrics[phase]

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
            threshold = REGRESSION_THRESHOLD
            if change > threshold:
                symbol = 'FAIL'
                regressions.append((phase, metric_name, change_pct, 'time'))
            elif change < -REGRESSION_THRESHOLD:
                symbol = 'IMPR'

            base_str = f"{base_val:.1f}"
            curr_str = f"{curr_val:.1f}"
            report_lines.append(
                f"  {phase:12} {metric_name:7} [{symbol:4}] {base_str:>7} -> {curr_str:>7} ({change_pct:+.1f}%)"
            )

    # Compare RAM metrics (RSS, peak_rss, steady_state_rss)
    if current_ram and 'ram_axis' in baseline:
        ram_baseline = baseline['ram_axis']

        # RSS (current RSS: median + p95)
        if 'rss' in ram_baseline and 'rss' in current_ram:
            base_rss = ram_baseline.get('rss', {})
            curr_rss = current_ram.get('rss', {})

            for metric_name in ['median', 'p95']:
                base_val = base_rss.get(metric_name, 0)
                curr_val = curr_rss.get(metric_name, 0)

                if base_val > 0 and curr_val > 0:
                    change = (curr_val - base_val) / base_val
                    change_pct = change * 100

                    symbol = 'OK'
                    if change > REGRESSION_THRESHOLD:
                        symbol = 'FAIL'
                        regressions.append(('RSS', metric_name, change_pct, 'ram'))
                    elif change < -REGRESSION_THRESHOLD:
                        symbol = 'IMPR'

                    report_lines.append(
                        f"  {'RSS':12} {metric_name:7} [{symbol:4}] {base_val:>7.2f} -> {curr_val:>7.2f} ({change_pct:+.1f}%)"
                    )

        # peak_rss (median + p95)
        if 'peak_rss' in ram_baseline and 'peak_rss' in current_ram:
            base_peak = ram_baseline.get('peak_rss', {})
            curr_peak = current_ram.get('peak_rss', {})

            for metric_name in ['median', 'p95']:
                base_val = base_peak.get(metric_name, 0)
                curr_val = curr_peak.get(metric_name, 0)

                if base_val > 0 and curr_val > 0:
                    change = (curr_val - base_val) / base_val
                    change_pct = change * 100

                    symbol = 'OK'
                    if change > REGRESSION_THRESHOLD:
                        symbol = 'FAIL'
                        regressions.append(('PEAK_RSS', metric_name, change_pct, 'ram'))
                    elif change < -REGRESSION_THRESHOLD:
                        symbol = 'IMPR'

                    report_lines.append(
                        f"  {'PEAK_RSS':12} {metric_name:7} [{symbol:4}] {base_val:>7.2f} -> {curr_val:>7.2f} ({change_pct:+.1f}%)"
                    )

        # steady_state_rss (scalar value)
        if 'steady_state_rss' in ram_baseline and 'steady_state_rss' in current_ram:
            base_steady = ram_baseline.get('steady_state_rss', 0)
            curr_steady = current_ram.get('steady_state_rss', 0)

            if base_steady > 0 and curr_steady > 0:
                change = (curr_steady - base_steady) / base_steady
                change_pct = change * 100

                symbol = 'OK'
                if change > REGRESSION_THRESHOLD:
                    symbol = 'FAIL'
                    regressions.append(('steady_state_rss', 'value', change_pct, 'ram'))
                elif change < -REGRESSION_THRESHOLD:
                    symbol = 'IMPR'

                report_lines.append(
                    f"  {'steady_state':12} {'value':7} [{symbol:4}] {base_steady:>7.2f} -> {curr_steady:>7.2f} ({change_pct:+.1f}%)"
                )

    passed = len(regressions) == 0
    report = "\n".join(report_lines)

    if regressions:
        report += "\n\n[REGRESSION] Detected:\n"
        for phase, metric, pct, kind in regressions:
            threshold_pct = TIER_TRANSITION_THRESHOLD * 100 if kind == 'tier' else REGRESSION_THRESHOLD * 100
            report += f"  {phase}.{metric}: +{pct:.1f}% (threshold: {threshold_pct:.0f}%)\n"

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
    current_metrics, current_ram = parse_bench_output(output)

    if not current_metrics:
        print("ERROR: Could not parse benchmark output", file=sys.stderr)
        print(output, file=sys.stderr)
        sys.exit(1)

    print("\nComparison (baseline -> current):")
    passed, report = compare_metrics(baseline, current_metrics, current_ram)
    print(report)

    if passed:
        print("\n[OK] No regressions detected")
        sys.exit(0)
    else:
        print("\n[FAIL] Regressions detected")
        sys.exit(1)

if __name__ == "__main__":
    main()
