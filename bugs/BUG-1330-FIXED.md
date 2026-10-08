# BUG-1330 — перевод строки между полноширинными (EAW = F/W) символами не удаляется, а превращается в пробел

**Статус:** FIXED 2026-10-08 (P6)
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

## Исправление

- Новый модуль `box_tree/segment_break.rs`: таблица East Asian Width F/W/H (бинарный поиск по диапазонам), признак Hangul, `segment_break_is_removed(before, after)` — правила §4.1.2: U+200B с любой стороны либо обе стороны F/W/H и ни одна не Hangul. `collapse_segment_breaks(&mut [InlineSegment])` убирает пробельные серии с `
` внутри сегмента и на стыке соседних сегментов (границы inline-боксов прозрачны; абсолютные и плавающие боксы сегментов не порождают).
- Вызов — один раз над всеми сегментами строки: в начале `split_inline_pieces`, в `build_anon_text_item` и в `build_ruby_group_box`. Только `normal`/`nowrap`; `pre-line`/`pre*` разрывы сохраняют (так что «normal|nowrap|pre-line» в заголовке — неточность: `pre-line` разрыв не схлопывает вовсе).
- Пробельный текстовый узел между inline-элементами (`<b>日本語</b>⏎<b>中国话</b>`) раньше сворачивался в `' '` на предыдущем сегменте и терял разрыв: теперь `folded_gap` оставляет `
`, если слева широкий символ или U+200B; латиница и прочее остаются с пробелом, дампы не меняются.
- Тесты: `box_tree/tests/segment_break.rs` (раскладка), юнит-тесты модуля. WPT: `seg-break-transformation-000…017` и `-018` проходят (143 проверки); `-019` не проходит из-за [BUG-1470](BUG-1470-OPEN.md) — `position:absolute`/`float` между `aa​` и `⏎bbb` разрывает строку, `bbb` уезжает на следующую, и дело уже не в разрыве.

