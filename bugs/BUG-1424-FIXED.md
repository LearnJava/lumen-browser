# BUG-1424 — Альфа сериализуется как `0.502`/`0.2`-квант вместо канонического `0.5`; проценты в `opacity` не нормализуются в число

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/js (сериализация цвета с альфой, `crates/engine/layout/src/selector_query.rs`, `web_api_shim_mid.js`)

## Симптом

`getComputedStyle(el).color` (`el.style.color = …` ставится, затем читается computed):

| значение | получено | ожидается (Chrome) |
|---|---|---|
| `rgb(0 128 0 / 0.5)` | `rgba(0, 128, 0, 0.502)` | `rgba(0, 128, 0, 0.5)` |
| `hsl(120 100% 25% / 0.5)` | `rgba(0, 128, 0, 0.502)` | `rgba(0, 128, 0, 0.5)` |
| `#00800080` | `rgba(0, 128, 0, 0.502)` | `rgba(0, 128, 0, 0.5)` |
| `background-color: rgb(0 0 0 / 0.2)` (контроль) | `rgba(0, 0, 0, 0.2)` | то же |

Альфа хранится как `u8` (128), и 128/255 = 0.50196 печатается тремя знаками. Chrome берёт кратчайшую запись из двух знаков, которая при
округлении даёт то же `u8` (0.5·255 = 127.5 → 128), и только потом три знака.

`style.opacity = "50%"` → `el.style.opacity` — `50%` (ожидается `0.5`; `<alpha-value>` процент канонизируется в число), `-100%` — `-100%`
(ожидается `-1`; computed — зажим 0, но specified-канон — `-1`).

## Как найдено

WPT-RUN-14 срез 19: 80 сабтестов в 13 файлах `css-color` (регэксп по сообщению `… 0.502)`); `opacity-valid.html` — 25 из 30
(`serialization should be canonical expected "0.5" but got "50%"`), `opacity-computed.html`.

## Что делать

Сериализовать альфу так: взять 8-битное значение, перебрать 2-, затем 3-значные десятичные записи и выбрать первую, что
округляется в то же значение. Процентную `<alpha-value>` в `opacity`/`rgb()` приводить к числу при записи в `style`.

## Как проверить

`css/css-color/parsing/color-computed.html`, `color-valid-hsl.html`, `opacity-valid.html`.

## Как исправлено

- `selector_query.rs::alpha_to_css` (через `color_to_css`, он же канон для `el.style` и computed): сначала двузначная запись
  `round(a/255·100)/100`, если она округляется обратно в то же `u8`, иначе трёхзначная. `128` → `0.5`, `51` → `0.2`, `127` → `0.498`.
- `web_api_shim_mid.js::_lumen_canonicalize_longhand`: для `opacity` процентное значение сохраняется числом (`50%` → `0.5`,
  `-100%` → `-1`, 6 значащих цифр). Остальные формы (в т. ч. `calc()`) идут как прежде.
- Тесты: `selector_query::tests::color_alpha_serialises_shortest_round_tripping_decimal`,
  `crates/js/src/dom/tests/v8_bug1424_alpha_opacity_serialization.rs`.
