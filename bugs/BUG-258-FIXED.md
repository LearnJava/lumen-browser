# BUG-258-FIXED.md

**Статус:** FIXED 2026-06-27

**Компонент:** paint (font metrics)

## Описание

After commit `51dbf9fd` (graphic regression: backdrop-filter edge-bleed fix, font-parity metrics, CI baseline), `FontMeasurer::descent_px` started computing descent as a proportion of `ascent_units + descent_units` instead of `units_per_em`, causing a sub-pixel shift in text baseline position.

## Test Results

- **TEST-02** (`color-named`): 0.68% → PASS
- **TEST-04** (`color-alpha`): 0.68% → PASS
- **TEST-56** (`mix-blend-mode`): 1.83% → PASS

## Root Cause

For Inter-Regular font:
- `units_per_em` = 2048
- `hhea` / `OS/2` descent = 494
- `hhea` / `OS/2` ascent = 1984
- `ascent + descent` = 2478

**Before 51dbf9fd:**
- `descent_px` = `494 / 2048 * font_size` ≈ 0.241 * font_size

**After 51dbf9fd:**
- `descent_px` = `494 / 2478 * font_size` ≈ 0.199 * font_size

The ~17% relative reduction in descent (from ~0.241 to ~0.199) caused text lines to shift upward relative to Edge.

## Fix

Reverted `descent_px` to compute via `units_per_em`:
```rust
fn descent_px(&self, font_size_px: f32) -> f32 {
    self.descent_units as f32 * font_size_px / self.units_per_em as f32
}
```

Kept `ascent_px` proportional (`ascent_units / total * font_size`), since for Inter its value (≈0.801 * font_size) is essentially identical to the old default 0.8 from `TextMeasurer`.
