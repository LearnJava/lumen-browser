# BUG-1321 — атрибут `dir` (`<div dir=rtl>`, `<bdo dir>`, `dir=auto`) не задаёт `direction`

**Статус:** FIXED 2026-10-07 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 9, `css/css-text`, первая половина)
**Область:** layout (`crates/engine/layout/src/style/ua.rs`, `style/presentational.rs` — для `dir` нет ни UA-правила, ни presentational hint; `style/matching/forms.rs:898` читает `dir` только для `:dir()`)

## Симптом

`--dump-layout` + `getComputedStyle` на странице:

```html
<div id=a dir=rtl style="width:300px">abc</div>      <!-- ожидается direction:rtl, текст у правого края -->
<div id=b dir=auto>אבג</div>                          <!-- rtl по первому сильному символу -->
<bdo id=c dir=rtl>abc</bdo>                           <!-- rtl + unicode-bidi:bidi-override -->
<p id=d dir=ltr>x</p>
<div id=e style="direction:rtl">x</div>
```

| элемент | `getComputedStyle().direction / unicodeBidi / textAlign` | `frag x` в `--dump-layout` |
|---|---|---|
| `a` (`dir=rtl`) | `ltr / normal / start` — ожидается `rtl / isolate / start` | 0,00 — ожидается у правого края (267,01) |
| `b` (`dir=auto`) | `ltr / normal / start` | 0,00 |
| `c` (`<bdo dir=rtl>`) | `ltr / normal / start` — ожидается `rtl / bidi-override` | — |
| `e` (`style=direction:rtl`) | `rtl / normal / start` | 267,01 — верно |

То есть механизм `direction` в раскладке работает (`e`), а HTML-атрибут `dir` его не включает (HTML LS §15.3.6 «Bidirectional text»: `[dir=ltr|rtl]` → `direction`, `unicode-bidi: isolate`; `bdo[dir]` → `bidi-override`; `dir=auto` — по первому сильному символу).

## Как найдено

WPT-RUN-14 срез 9, `css/css-text`: 90 не зелёных id содержат атрибут `dir` в тексте теста (`text-align/` 41, `shaping/` 38, `hyphens/` 5, `overflow-wrap/` 2, `bidi/`, `boundary-shaping/`, `letter-spacing/`, `line-break/` по одному). Пиксельный разбор (`reftest_pixdiff.py --viewport 800x600 --ahem`): `text-align-start-003/007/009/014…017`, `text-align-end-003/007/009/010/014/016` — 16 reftest в кластере «text-align + dir» (13 thick, 3 no-match-ref), остальные из 90 лежат в других кластерах (`shaping/` — эталоны и тесты Noto) и сначала упираются в свою причину. Тесты `text-align-start-001/002/004…` («direction: rtl» стилем) от атрибута не зависят. Сколько из 90 станут зелёными после правки — не проверено.

## Что делать

UA-таблица (`ua.rs`) и/или presentational hints (`presentational.rs`): `[dir=ltr]`/`[dir=rtl]` → `direction` + `unicode-bidi: isolate`; `bdo[dir]` → `unicode-bidi: bidi-override`; `[dir=auto]` → первый сильный символ текста элемента (UAX #9 P2/P3) — выставляется при построении box tree, не в каскаде. `<html dir=rtl>` наследуется по обычным правилам.

## Как проверить

`css/css-text/text-align/text-align-start-003.html`, `text-align-end-003.html` (`dir=rtl`), `text-align-start-009.html` (`dir=auto`); проба выше одной строкой `getComputedStyle(document.querySelector('[dir=rtl]')).direction === 'rtl'`.

## Исправление

`crates/engine/layout/src/style/presentational.rs`: `apply_dir_presentational_hint()` (вызов в `cascade.rs` рядом с `align`-хинтом, до author-каскада — author `direction`/`unicode-bidi` выигрывает). `dir=ltr|rtl` (без учёта регистра) → `direction` + `unicode-bidi: isolate`; `<bdo dir>` → `isolate-override` (HTML Rendering §15.3.6 — в текущей редакции вместо `bidi-override` из «Что делать»); `dir=auto` → `first_strong_direction()`: первый символ класса L/R/AL в тексте поддерева, пропуская `script`/`style`/`textarea`/`bdi` и потомков с собственным `dir`; без сильных символов — `ltr`. Неизвестное значение игнорируется. `:dir()` в `matching/forms.rs` по-прежнему считает `auto` за `ltr` — отдельная доработка, к этому багу не относится.

Проверка: `style::tests::ua::dir_attr_*` (7 тестов); проба из «Симптома» в `--dump-layout`: `dir=rtl` — `frag x=277,80`, `dir=auto` с ивритом — 279,65, `dir=ltr` — 0,00, `style=direction:rtl` — 292,00. Полный `graphic_tests/run.py` на свежем бинаре — набор FAIL тот же, что до правки (12 известных). Сколько из 90 WPT-id станут зелёными — не проверялось (следующий прогон `css/css-text`).
