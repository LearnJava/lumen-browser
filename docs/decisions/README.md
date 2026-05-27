# Architectural Decision Records

Formal ADR files for Lumen. Each file covers one decision: context, what we decided, alternatives considered, consequences.

For new decisions use [TEMPLATE.md](TEMPLATE.md). Numbering is sequential; do not reuse numbers.

**Historical decisions** (pre-ADR format, unstructured) — [DECISIONS.md](../../DECISIONS.md) at repo root. Do not add new decisions there; use this directory instead.

---

## Index

| # | Title | Status | Date |
|---|---|---|---|
| [ADR-001](ADR-001-custom-rendering-engine.md) | Custom rendering engine (not a browser wrapper) | Accepted | 2026-05-01 |
| [ADR-002](ADR-002-dependency-policy.md) | Two-tier dependency policy (permanent + provisional) | Accepted | 2026-05-15 |
| [ADR-003](ADR-003-sqlite-storage.md) | SQLite for all persistent browser storage | Accepted | 2026-05-20 |
| [ADR-004](ADR-004-js-runtime.md) | rquickjs (QuickJS) as Phase 0 JS engine, rusty_v8 for v1.0+ | Accepted | 2026-05-20 |
| [ADR-005](ADR-005-image-decoding.md) | zune-jpeg + zune-png as provisional image decoders | Accepted | 2026-05-22 |
| [ADR-006](ADR-006-automation-api.md) | Automation API as a first-class engine surface (BrowserSession trait, in-process + MCP + WebDriver BiDi) | Accepted | 2026-05-27 |
| [ADR-007](ADR-007-anti-detection-stack.md) | Anti-detection as a privacy stack, not a circumvention tool (TLS/HTTP/surface fingerprint defaults; no CAPTCHA-solver, no IP rotation) | Accepted | 2026-05-27 |
| [ADR-008](ADR-008-tab-lifecycle-memory-tiers.md) | Tab lifecycle and memory tiers (T0–T4 model; DOM arena / JS suspend / pure layout invariants; per-tier RAM budgets and restore SLOs) | Accepted | 2026-05-27 |
