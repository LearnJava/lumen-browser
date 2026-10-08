# BUG-1326 — `tab-size`: табуляция — фиксированные N × 8 px после текста, а не до следующей позиции табуляции в единицах ширины пробела

**Статус:** OPEN
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

`white-space/` — ещё 20 не зелёных id про табуляцию (`break-spaces-tab-*`, `pre-wrap-tab-*`, `tab-stop-threshold-*`, `tab-bidi-001`, `tab-position-with-text-align`, `text-indent-tab-positions-001`), 16 из них с `pre-wrap`/`break-spaces`. Проба: `<i>a</i>	<i>b</i>` в `white-space: pre` — `x` второго элемента 11 (табуляция между элементами пропадает целиком, см. [BUG-1327](BUG-1327-OPEN.md)); `a	<i>b</i>` — 75 (N × 8 px).
