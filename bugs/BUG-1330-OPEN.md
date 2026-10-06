# BUG-1330 — перевод строки между полноширинными (EAW = F/W) символами не удаляется, а превращается в пробел

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` / `inline_build.rs` — схлопывание сегментных разрывов в `normal|nowrap|pre-line`; CSS Text L3 §4.1.2 «Segment Break Transformation Rules»)

## Симптом

`<div><span>ＦＵＬＬ⏎ＷＩＤＴＨ</span></div>` (`⏎` = U+000A, `font: 24px sans-serif`): `--dump-layout` — фрагмент `ＦＵＬＬ ＷＩＤＴＨ` (с пробелом), у `<span>ＦＵＬＬＷＩＤＴＨ</span>` — без. Латиница `FULL⏎WIDTH` → `FULL WIDTH` верно. По правилам перевод строки между символами EAW = F/W/H (не Hangul) удаляется, пробелы вокруг него тоже.

## Как найдено

WPT-RUN-14 срез 10: `white-space/seg-break-transformation-001…004, 008, 009, 016, 017.tentative.html` (testharness, `offsetWidth` против эталона без перевода строки: `expected 141.75 but got 146.25`; 8 id / 54 сабтеста) и `-018/-019.tentative.html` (2 reftest, `no-match-ref`).

## Что делать

При построении InlineSegment применить таблицу §4.1.2: удалять разрыв, если по обе стороны символы F/W/H (кроме Hangul); для Hangul/латиницы — оставлять пробел. Пробелы вокруг разрыва убирать до применения правила.

## Как проверить

`css/css-text/white-space/seg-break-transformation-001.tentative.html`.
