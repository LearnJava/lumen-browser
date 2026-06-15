# Benchmarking & Comparison Strategy

Thoughts on how to measure and communicate Lumen's performance relative to other browsers.
Not a formal ADR — no decision is made here. This is a reference for prioritizing benchmark work.

---

## Core principle

Raw pipeline microseconds mean nothing to users. Comparisons must be grounded in
**real-world scenarios** with numbers a non-engineer can interpret.

---

## Dimensions worth measuring

### 1. Memory per tab (highest impact)

Open the same N tabs in Chrome, Firefox, and Lumen; record RSS.

```
Chrome  (20 tabs):  ~3.2 GB
Firefox (20 tabs):  ~2.1 GB
Lumen   (20 tabs):  ~600 MB   ← Phase 2 target (ADR-008 T2/T3 tiers)
```

Most compelling for users whose laptops slow down with many tabs open.
Measurable with `lumen-bench` RSS stats + `/usr/bin/time -v` or Task Manager on Windows.

### 2. Cold start time

Time from process launch to first painted pixel.

```
Chrome: ~800ms–1.5s  (profile load, GPU process spawn, V8 snapshot)
Lumen:  ~50–100ms    (single process, no extensions, no V8 warmup)
```

Chrome carries 20 years of legacy startup cost. Lumen wins structurally, not by optimization.
Measure: `time cargo run -p lumen-shell -- samples/page.html 2>/dev/null`.

### 3. Automation / test suite speed

Unique to Lumen's architecture (ADR-006): in-process `BrowserSession` vs CDP round-trip.

```
Playwright + Chrome headless:  ~800ms per action (HTTP round-trip per CDP command)
lumen-driver in-process:       ~8ms per action   (direct Rust function call)
```

For a team with 500 end-to-end tests this is the difference between 10-minute CI and 6-second CI.
Strongest selling point for developer adoption.
Measure: same test suite run via both drivers, wall-clock time.

### 4. Privacy / fingerprint entropy

Run fingerprintjs or similar entropy estimator against Chrome and Lumen.

```
Chrome:  ~18 bits entropy  (uniquely identifiable among ~250k users)
Lumen:   ~4  bits entropy  (Canvas noise, no WebGL fingerprint, Battery API disabled)
```

Show which fields leak in Chrome vs which are suppressed in Lumen (ADR-007 anti-detection stack).
Users understand "they know who you are" vs "they don't" better than entropy numbers.

### 5. Real page — side by side

Same URL loaded simultaneously in Chrome and Lumen, screen-recorded:
- Time to first text
- Time to full paint
- Count of tracking requests blocked (Lumen built-in adblock vs Chrome without extensions)

Good for demos and blog posts. Requires streaming render pipeline (Phase 1).

### 6. Tab lifecycle under pressure

Open 50 tabs, leave for 1 hour, return:

```
Chrome:   tabs silently discarded, scroll position lost, forms cleared
Lumen T3: tabs hibernated to SQLite (~200 KB/tab), restore < 200ms,
          scroll position preserved, form state intact
```

This is a pain point every power user has felt. No other browser has addressed it at the
engine level. Measurable: RAM usage over time + restore latency per tab tier.

---

## What internal benchmarks cover (`lumen-bench`)

`cargo run -p lumen-bench --release` measures Lumen's own pipeline phases:

```
decode → parse_html → parse_css → layout → paint
```

Outputs min / median / mean / p95 / max per phase + RSS (Resident Set Size = physical RAM used).
`baseline.json` stores reference numbers; `--ci` flag exits non-zero on regression.

These are **regression guards**, not cross-browser comparisons.

---

## Industry standard benchmarks (when applicable)

| Benchmark | What it measures | When Lumen can run it |
|---|---|---|
| MotionMark | CSS rendering / animation throughput | Phase 1 (no JS required for CSS subset) |
| Speedometer 3 | UI responsiveness with real JS frameworks | Phase 2 (QuickJS wired) |
| JetStream 2 | JS + WebAssembly throughput | Phase 3 (V8) |
| Kraken | JS microbenchmarks | Phase 2–3 |
| WPT (web-platform-tests.org) | Spec compliance % per module | Phase 2+ (custom runner over `lumen-driver` in-process API) |

WPT standard runner expects WebDriver BiDi. `lumen-bidi-server` (task 8H) is opt-in and will only
be built on real demand. Alternative: write a custom WPT runner in Rust that drives Lumen via
`lumen-driver` in-process API directly — faster, no network layer, no BiDi dependency.
WPT pass rate target: ≥ 60% by v1.0 (Phase 3).

---

## Web Vitals — user-perceived performance

| Metric | Meaning |
|---|---|
| LCP (Largest Contentful Paint) | When the main content appeared |
| INP (Interaction to Next Paint) | Latency from click to screen update |
| CLS (Cumulative Layout Shift) | How much elements "jump" during load |
| TTFB (Time To First Byte) | Network + server response time |

These are meaningful for real-page comparisons once streaming render is in place (Phase 1).

---

## External resources for tracking standards & performance

| Resource | Purpose |
|---|---|
| [caniuse.com](https://caniuse.com/) | Usage % + browser support matrix — prioritize by real adoption |
| [webstatus.dev](https://webstatus.dev/) | Baseline Widely Available: supported in all browsers for 2+ years |
| [chromestatus.com](https://chromestatus.com/) | Chrome intent to ship/deprecate — earliest signal |
| [wpt.fyi](https://wpt.fyi/) | WPT results per browser per test — see exactly where Lumen diverges |
| [web.dev/blog](https://web.dev/blog/) | Google DevRel: new CSS/JS features with implementation notes |
| [drafts.csswg.org](https://drafts.csswg.org/) | CSS WG drafts before W3C publication |
| [spec.whatwg.org](https://spec.whatwg.org/) | Living standards: HTML, DOM, Fetch, URL, Streams |
| [tc39.es/proposals](https://tc39.es/proposals/) | JS proposals Stage 1–4; Stage 4 = in standard |
| [blink-dev mailing list](https://groups.google.com/a/chromium.org/g/blink-dev) | Earliest signal: Intent to Prototype/Ship in Chromium |

When a feature reaches **Baseline Widely Available** (2+ years in all browsers) — sites actively
rely on it and a browser without it will break pages.

---

## Roadmap for benchmark work

| Phase | What to implement |
|---|---|
| Phase 0–1 (now) | `lumen-bench` regression gate (done), cold start timing, RSS-per-tab tracking |
| Phase 1 | LCP / TTFB on real pages, MotionMark CSS subset, side-by-side demo tooling |
| Phase 2 | Tab lifecycle memory comparison (50 tabs), Speedometer, custom WPT runner via lumen-driver |
| Phase 3 | JetStream, WPT pass rate dashboard, full Web Vitals suite |
