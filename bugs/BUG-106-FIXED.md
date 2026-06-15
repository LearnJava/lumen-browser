# BUG-106

**Статус:** FIXED 2026-06-09
**Компонент:** layout
**Файл:** `crates/engine/layout/src/style.rs`

## Описание

TEST-64 table 24.85%→14.90%. Dominant cause was NOT table layout but missing UA heading defaults: `<h3>` rendered at 16px with no margins, so both tables sat ~25px too high vs Edge (offset compounded down the page). Fix: apply_ua_heading_style (style.rs) sets UA font-size (h1 2em…h6 0.67em) + vertical margins (em of own font-size, HTML Rendering §15.3.3); author font-size overrides via pre-pass, author margin via main-pass. Residual ~15% is content-based auto table column widths (Lumen splits available width equally, Edge sizes columns to content) — filed BUG-116.
