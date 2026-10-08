# BUG-1326 — `tab-size`: табуляция — фиксированные N × 8 px после текста, а не до следующей позиции табуляции в единицах ширины пробела

**Статус:** FIXED 2026-10-08 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 9, `css/css-text`, первая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` — измеритель `measure_text_w_varied(…, tab_size, …)` и ветка `preserves_whitespace()` (:600); `style/apply/text.rs` — разбор `tab-size`)

## Симптом

`<pre style="font:20px monospace;tab-size:N">…</pre>`, `--dump-layout`, `x` фрагмента `b` после `\t` (ширина пробела `monospace` 20px — 11 px):

| текст до табуляции | `tab-size` | получено | ожидается (CSS Text L3 §4.2) |
|---|---|---|---|
| `a` (11) | `4` | 43 (= 11 + 32) | 44 (до позиции 4 × 11) |
| `a` | `8` | 75 (= 11 + 64) | 88 |
| `abcdefgh` (88) | `8` | 152 (= 88 + 64) | 176 (следующая позиция 8 × 11 = 88 уже занята — до 176) |
| `abcdefgh` + два `\t` | `8` | 216 (= 88 + 2 × 64) | 264 |
| `ab` (22) | `2` | 38 (= 22 + 16) | 22 (позиция 2 × 11 = 22 уже занята — до 44) |
| `a` | `40px` | 51 (= 11 + 40) | 40 |
| `a` | `80px` | 91 (= 11 + 80) | 80 |
| `a`, `font-size: 40px` | `4` | 54 (= 22 + 32) | 88 (4 × 22) |

Табуляция всегда добавляет к ширине предшествующего текста **N × 8 px** (число без единиц) или N px (длина), не выравнивая по позиции табуляции и не умножая на ширину пробела (`tab-size: 4` при `font-size: 40px` даёт те же 32 px, что и при 20px).

## Как найдено

WPT-RUN-14 срез 9: `css/css-text/tab-size/` — 10 reftest `thick` (`tab-size-block-ancestor`, `-inheritance-001`, `-inline-001/002`, `-integer-004/005`, `-spacing-001/002/003`, `tab-min-rendered-width-1`) + `tab-size.html` (testharness: ожидается 60, получено 0). Пробой подтверждена только таблица выше; `tab-size-computed-value-001.html` (11 сабтестов, `getComputedStyle().tabSize` — `""`) отнесён к [BUG-1325](BUG-1325-FIXED.md).

## Что делать

Ширина табуляции = расстояние до следующей позиции, кратной `tab-size × ширина U+0020` (для длины — кратной длине), от начала строки (inline-контейнера), с минимумом ширины в один пробел (`tab-min-rendered-width`, CSS Text L3 §4.2: если до позиции меньше половины ширины пробела — переход на следующую). Учитывать `letter-spacing`/`word-spacing` (`tab-size-spacing-*`) и наследование от block-предка (`tab-size-block-ancestor`).

## Как проверить

`css/css-text/tab-size/tab-size-integer-004.html`, `tab-size.html`; проба выше (`--dump-layout`).

## Дополнение: WPT-RUN-14 срез 10 (2026-10-06, `css/css-text`, часть 2)

`white-space/` — ещё 20 не зелёных id про табуляцию (`break-spaces-tab-*`, `pre-wrap-tab-*`, `tab-stop-threshold-*`, `tab-bidi-001`, `tab-position-with-text-align`, `text-indent-tab-positions-001`), 16 из них с `pre-wrap`/`break-spaces`. Проба: `<i>a</i>	<i>b</i>` в `white-space: pre` — `x` второго элемента 11 (табуляция между элементами пропадает целиком, см. [BUG-1327](BUG-1327-FIXED.md)); `a	<i>b</i>` — 75 (N × 8 px).

## Причина и исправление

Ширина табуляции считалась как `tab_size` px для каждого `\t` внутри `measure_text_w_varied`, а число из `tab-size: N` на каскаде превращалось в `N × 8` px — без позиции на строке и без ширины пробела. Теперь:

* `TabStops` (`inline_wrap.rs`) — шаг табуляции: для числа `N × (ширина U+0020 + letter-spacing + word-spacing)`, для длины — она сама; минимальная отрисованная ширина — `0.5ch` (меньший остаток пропускает позицию).
* `measure_text_tabbed` и `push_tabbed_frags` — табуляция идёт до следующей позиции от начала строки (с учётом `text-indent` и предыдущих фрагментов); каждая табуляция — отдельный `InlineFrag` с реальной шириной в `style.tab_size`, поэтому paint рисует её той же шириной и не менялся. Применено в ветке `white-space: pre` и в `pre-wrap`/`break-spaces` (`inline_wrap_preserved.rs`; решение о переносе меряет токены с реальной позиции).
* Внутренние ширины (`intrinsic.rs`, маркер списка) брали `tab_size × ширина пробела` — то есть px, умноженные на пробел; теперь берут шаг `TabStops::unit`.

Тесты: `box_tree/tests/tab_size.rs` (9 штук: все строки таблицы симптомов, `pre-wrap`, минимальная ширина, сброс на новой строке), `pre_element_tab_renders_with_tab_size` обновлён под три фрагмента (текст / табуляция / текст).

**Остаток — [BUG-1461](BUG-1461-OPEN.md):** метрики пробела берутся у стиля сегмента, а CSS Text L3 §4.2 требует у блочного контейнера (`tab-size-integer-004`). WPT-прогон `tab-size/` после правки не делался (нужен WPT-venv); по коду должны позеленеть те reftest, где контейнер и сегмент не расходятся по шрифту и интервалам.
