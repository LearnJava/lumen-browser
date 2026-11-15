# BUG-1408 — CSSOM `element.style`: шрифтовые и масочные свойства принимают невалидное и не канонизируют валидное

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** js/layout (`crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand` — нет грамматики для `font-*`, `mask-*`, `clip-*`)

## Симптом

`t.style.cssText = ''; t.style[prop] = val; t.style[prop]`:

| присваивание | получено | ожидается |
|---|---|---|
| `fontKerning = 'bogus'` | `bogus` | `""` |
| `fontSizeAdjust = 'bogus'` | `bogus` | `""` |
| `fontWeight = 'bogus'` | `bogus` | `""` |
| `fontFamily = 'cursive serif'` | `cursive serif` | `""` |
| `fontWeight = 'calc(100 + 100)'` | `calc(100 + 100)` | `calc(200)` |
| `maskType = 'bogus'` | `bogus` | `""` |
| `maskRepeat = 'repeat repeat'` | `repeat repeat` | `repeat` |
| `maskSize = '1px auto'` | `1px auto` | `1px` |
| `clipPath = 'bogus'` | `bogus` | `""` |
| `fontLanguageOverride = '"ENG"'` | `"ENG"` | `"ENG "` (канон с пробелом до 4 символов — по тесту `font-language-override-valid`) |

Значения не проходят через настоящий CSS-парсер: сеттер хранит строку (BUG-484, закрыт для базового набора
свойств, но таблица грамматики не покрывает `font-*`/`mask-*`/`clip-*`).

## Как найдено

WPT-RUN-14 срез 18: `css-fonts/parsing/*-invalid.html` (25 файлов) и `*-valid.html` (`font-valid`, `font-weight-valid`,
`font-width-valid`, `font-style-valid`, `font-synthesis-valid`, `font-variant-valid`, `font-size-adjust-valid`,
`font-feature-settings-valid`, `font-variation-settings-valid`, `font-language-override-valid`,
`font-variant-east-asian-valid`, `font-family-valid`), `css-masking/parsing/*-invalid.html` (9),
`clip-path-valid`, `clip-path-shape-parsing`, `mask-valid.sub`, `mask-position-valid`, `mask-repeat-valid`,
`mask-size-valid`.

## Что делать

Расширить `_lumen_canonicalize_longhand` таблицей грамматики для `font-*`, `mask-*`, `clip`, `clip-path`, `clip-rule`;
для свойств, которых нет в парсере (`font-kerning`, `font-synthesis`, …), валидатор появится вместе с самим
свойством (`CSS-SPECS.md`).

## Как проверить

`css/css-fonts/parsing/font-weight-invalid.html`, `font-weight-valid.html`, `css/css-masking/parsing/mask-repeat-invalid.html`,
`mask-size-valid.html`.

## Дополнение WPT-RUN-14 срез 19 (2026-10-08)

Тот же механизм у свойств `css-images`: `el.style.objectFit = "bogus"` и `el.style.imageRendering = "bogus"` читаются назад как записаны (проба,
setter-страница из `getComputedStyle`/`style`); `style.objectPosition = "10%"` читается `10%` (канон `10% center`). Затронуто (по сообщениям):
`css-images/parsing/object-fit-{invalid,valid,computed}.html`, `object-position-{invalid,valid,computed}.html`, `image-rendering-invalid.html` —
около 40 сабтестов из 546 «images прочее».

## Срез 1 (2026-11-16, P3): CSS Masking

Валидация и канон `element.style` для `mask-*` (`crates/engine/layout/src/style/values/mask_cssom.rs`,
`canonical_specified_mask`, натив `_lumen_css_canonical_mask`, таблица `_LUMEN_MASK_PROPERTIES`):
`mask-{image,size,repeat,position}` — та же грамматика, что `background-*` (делегирование
`canonical_specified_background`), `mask-origin`/`mask-clip` — `<coord-box>`/`no-clip`,
`mask-composite`, `mask-mode`; `mask-type`/`clip-rule` — одиночные ключевые слова через
`_LUMEN_KEYWORD_PROPERTIES`. Трёхзначная `<position>` для `mask-position` отвергается
(`right top 5px` и др. — `mask-position-invalid`), в отличие от `background-position`, где та же
форма принимается. Попутно: `setProperty` со значением, содержащим `;` вне кавычек/скобок, теперь
отбрасывает объявление (CSSOM §6.7.4 «parse a CSS value») — раньше `mask-repeat = "repeat; round"`
усекалось до `repeat` при записи в `style=""`.

Зелёные файлы (`css/css-masking/parsing/`, ожидания `.ini` удалены): `mask-repeat-{invalid,valid}`,
`mask-size-{invalid,valid}`, `mask-position-{invalid,valid}`, `mask-composite-{invalid,valid}`,
`mask-type-{invalid,valid}`, `clip-rule-{invalid,valid}` — 12 файлов, 0 неожиданных результатов.

**Остаток среза:** шорткод `mask` (`mask-valid.sub.html`, `mask-invalid.html`) — нужна
раскладка `<mask-layer>` по лонгхендам и сборка обратно; `clip`/`clip-path`
(`clip-{invalid,valid,computed}.html`, `clip-path-*.html`) — `parse_clip_path` сводит
`rect()`/`xywh()` к `inset()` и не является канонизатором указанного значения, нужен свой
сериализатор; `css-fonts`-половина карточки (шрифтовые свойства) не тронута.
