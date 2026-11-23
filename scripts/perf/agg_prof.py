# -*- coding: utf-8 -*-
import re, sys
from collections import defaultdict
sys.stdout.reconfigure(encoding="utf-8")
agg = defaultdict(float)
top_single = []
pat = re.compile(r"\[profile\]\s+([0-9.]+)ms\s+(.*)")
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    m = pat.search(line)
    if not m: continue
    ms = float(m.group(1)); name = m.group(2).strip()
    base = re.sub(r"\s*×\d+$", "", name)
    agg[base] += ms
for k, v in sorted(agg.items(), key=lambda kv: -kv[1])[:35]:
    print(f"{v:10.0f}ms  {k}")
