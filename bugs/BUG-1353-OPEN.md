# BUG-1353 — Строчный бокс, разрезанный блоком («block-in-inline», CSS 2.1 §9.2.1.1): рамка у фрагментов не по правилу `slice`, а первый фрагмент пропадает, если элемент начинается с блока

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 12, `css/CSS2` (normal-flow + margin-padding-clear))
**Область:** layout/paint (`crates/engine/layout/src/box_tree/inline_build.rs`, `inline_wrap.rs` — строчный бокс, разрезанный блоком)

## Симптом

`--dump-display-list`: `<span style="border:3px solid blue">a<div>One</div>b</span>` → два `DrawBorder` с **четырьмя** рамками (`(0,0,15,16)` и `(-3,32,16,16)`); у первого фрагмента не должно быть правой рамки, у второго — левой (второй уходит в минус по `x`). `<span style="border:3px solid blue"><div>One</div><span>Two</span></span>` → **ни одного** `DrawBorder`: пустой первый фрагмент (рамка слева, сверху, снизу) не рисуется.

## Как найдено

WPT-RUN-14 срез 12: 21 id `normal-flow/block-in-inline-*` (+ `margin-right-114.html`) не зелёных в pixdiff. **Ещё 24 id — сами файлы `block-in-inline-{append,insert,remove}-*-ref.xht`**: у них в манифесте есть `rel=match` на `-nosplit-ref.xht`, `wptrunner` считает их reftest-ами, и все 24 FAIL; в `reftest_pixdiff.py` они не попали (скрипт пропускает `*-ref.xht`). Разбор этих 24 не делался — по сути та же пара «разрез против не-разреза». `block-in-inline-insert-*` и `-remove-*` запускают JS в `onload`; через CLI `--dump-display-list` снимок делается до `onload`, поэтому проверить, закрывает ли правка и их, нельзя.

## Что делать

Для каждого фрагмента разрезанного строчного бокса выдавать рамку по `slice`: первому — без правой, последнему — без левой, средним — без обеих; пустой фрагмент с рамкой/padding не отбрасывать.

## Как проверить

`css/CSS2/normal-flow/block-in-inline-append-002-ref.xht`, `block-in-inline-first-line-001.html`.

## Срез 15 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` остальное)

`css/CSS2/box-display/block-in-inline-001/002/007.xht`, `block-in-inline-relpos-001/002.xht`, `block-in-inline-self-collapsing-only-child.html` — 9 id `thick` с `<span class="inline">…<span class="block">…</span>…</span>` (`display:block` внутри `display:inline`). Проба на этих id не выполнялась; единственное, что проверено: `<span class=inline>Line 1<span class=block>Line 2</span>Line 3</span>` (`--dump-layout`) даёт `InlineRun "Line 1"` + `Block` (зелёный, `Line 2`) + продолжение — деление на блоки есть, что с фоном/рамкой у фрагментов `inline` — не смотрели. Привязка к BUG-1353 — по сходству разметки.
