# BUG-1779 — `Object.prototype.toString.call(<HTML-элемент>)` возвращает `[object Object]`

**Статус:** FIXED 2026-11-14 (P3)
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js (обёртки DOM-узлов, `Symbol.toStringTag`/классификация хост-объектов)

## Источник: сайты

mirtesen-ru (вес 3, `text_len` 2 при 9812 у Chrome), aviasales-ru (вес 2, 1 при 9026), smi2-ru (вес 3, SITES-TRIAGE-3 2026-11-13: обе попытки DEGRADED, текст 102 при 9443–9448 у Chrome, stderr `SyntaxError: [object Object] is not a valid selector` + `adfox#checkContainer`) — **3 сайта, вес кластера 8**. Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank и §Разбор зависаний. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси.

Живой стек mirtesen: `_lumen_sel` ← `HTMLDocument.querySelectorAll` ← `Fc` (tippy.js, `packages-B_Rt485v.js:12`) ← `initTooltip` ← `componentDidMount` → `SyntaxError: [object Object] is not a valid selector`, React-приложение не монтируется, страница пуста.

## Причина (подтверждена)

tippy.js определяет «это элемент» через `{}.toString.call(x)` и ищет подстроку `Element]`. В Lumen для `div/span/input/html/head/Text/Comment/DocumentFragment/canvas/video` результат — `[object Object]` (SVG-элементы дают верное `[object SVGSVGElement]`, `document` — `[object HTMLDocument]`, `NodeList` — `[object NodeList]`). Элемент не узнаётся и уходит в `document.querySelectorAll(<элемент>)`.

## Минимальная репродукция

```html
<!doctype html><div id=a></div><pre id=out></pre>
<script>out.textContent = {}.toString.call(a) + ' ' + {}.toString.call(document.head) + ' ' + {}.toString.call(document.createTextNode(''));</script>
```

`lumen --dump-layout` такого файла печатает `[object Object]` для каждого элемента.

## Ожидание (Chrome)

`[object HTMLDivElement] [object HTMLHeadElement] [object Text]` (`Symbol.toStringTag` на прототипах интерфейсов: `HTMLDivElement`, `Text`, `Comment`, `DocumentFragment`, `HTMLHeadElement`, …).

## Где искать

Прототипы обёрток DOM-узлов в `crates/js` (V8-обёртки и shim `web_api_shim_*.js`): у SVG-элементов тег уже есть — свести остальные к той же схеме.

## Как проверить

Репродукция выводит `[object HTMLDivElement] …`; mirtesen.ru и aviasales.ru в живом окне не пишут `is not a valid selector`.

## Фикс (P3, 2026-11-14) — каждый интерфейсный прототип несёт собственный `@@toStringTag`

**Механизм:** домовой помощник `_lumen_idl_tag` (форма BUG-912/BUG-1138 — тег обязан быть
per-class, унаследованный заставил бы подкласс носить имя базы; WebIDL §3.7.3) применён ко
всем обёрткам DOM-узлов: node-семейство (`Node`/`Element`/`CharacterData`/`Attr`/
`DocumentType`/`ProcessingInstruction`/`HTMLElement`/`Text`/`Comment`/`CDATASection`/
`DocumentFragment`), оба генерируемых списка конкретных HTML-интерфейсов
(`web_api_shim_mid.js` 39 имён, `web_api_shim_tail_b.js` 25), одиночные `HTMLImageElement`
и `HTMLMediaElement` (`video_element.js`), `MathMLElement` (`mathml.rs` — класс без тега
строкофицировался бы как унаследованный `Element`). Ближайший собственный тег выигрывает:
конкретный элемент называет свой интерфейс, элемент без конкретного интерфейса —
`HTMLElement`, инстанс кастомного элемента наследует тег `HTMLElement.prototype` — parity с
Chrome на всех трёх формах. SVG не тронут (`svg_idl_shape.js` теговал и раньше), события —
BUG-912, `document`/`XMLDocument`/`Document` — BUG-1138.

**Проба «до»** (20 значений, `--dump-layout`): сломаны все HTML-элементы (включая
`section`→`HTMLElement` и неизвестный тег→`HTMLUnknownElement`), `Text`/`Comment`/
`DocumentFragment`/`Attr`/`DocumentType`, элемент XML-документа (`Element`) и
`ProcessingInstruction`; верны были только SVG, `document`/`XMLDocument` и `NodeList`.
Проба нашла соседа НЕ этой формы: `document.implementation.createHTMLDocument()` возвращает
`[object Document]`, у Chrome `[object HTMLDocument]` — неверный интерфейс у фабрики, теги
не при чём; заведено [BUG-1808](BUG-1808-OPEN.md).

**Тесты:** `v8_bug1779_element_class_string.rs` (4) — репро карточки + сниф tippy
(`/Element]/`), по-инстансовые имена всей семьи (включая `MathMLElement`,
`HTMLMediaElement`, кастомный элемент), ближайший тег на цепочке (XML-элемент → `Element`,
PI/CDATASection), собственные дескрипторы на каждом прототипе (форма BUG-912). Полный
lib-прогон `cargo test -p lumen-js --features v8-backend --lib`: 5008 зелёных, 3 красных —
те же, что на чистом HEAD (парный полный прогон до правки: те же 3 + флака `perf9`
кэша; `css_float_and_border_shorthand_round_trip` — BUG-1795, clipboard/permissions —
флака settle-микротасков под нагрузкой, соло зелёные на обоих деревьях).

**Гейты:** clippy `-p lumen-js --features v8-backend --all-targets -D warnings` чистый
(без фичи `--all-targets` не компилируется — pre-existing, BUG-1804); graphic-гейт
7/172 FAIL = ровно набор срезов 5 BUG-1800 (21, 56, 62, 150, 151, 155, 159), дельта к
парному прогону HEAD — только калибровка TEST-00, пиксели не тронуты
(`graphic_tests/results/latest.json`).

**Живая сверка** (`compat_crawl.py`, живое окно, `LUMEN_NO_ADBLOCK=1`, корпус
`corpus-ab.txt`, `.tmp/compat-crawl/20261114-042202`): `is not a valid selector` — 0
вхождений на всех трёх сайтах. mirtesen-ru: текст 2 → 6891–6994 (у Chrome 7028–7037),
React-приложение монтируется, единственное исключение — несвязанный пробел Battery
Status API; smi2-ru: 102 → 1594–1602 (у Chrome 8861–9488), статус DEGRADED → **OK**,
остаток — a11y-дифф; aviasales-ru: 1 → 1762 (у Chrome 1925), PARITY, второй прогон
дал blank (флап загрузки, вне бага). Критерий карточки выполнен — баг закрыт.

**Осталось:** [BUG-1808](BUG-1808-OPEN.md) (createHTMLDocument → неверный интерфейс);
остаток smi2-ru (текст 1.6k против 8.9–9.5k у Chrome при 0 исключений) — отдельный
разбор, не toStringTag.
