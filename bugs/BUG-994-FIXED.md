# BUG-994 — у коллекций DOM нет `item()`: `document.getElementsByTagName(…).item is not a function`

**Статус:** FIXED
**Заведён:** 2026-09-04 (живой прогон корпуса «топ-100 зарубежных»)
**Закрыт:** 2026-09-05 (P3)
**Область:** `crates/js/src/shim/web_api_shim_mid.js` — фабрики коллекций (`getElementsByTagName` / `getElementsByClassName` / `querySelectorAll` / `children` / `childNodes`)
**Владелец:** P3

## Симптом

```
Uncaught TypeError: document.getElementsByTagName(...).item is not a function
Uncaught TypeError: a.item is not a function
Uncaught TypeError: t.item is not a function
```

Коллекция вела себя как массив: индексный доступ и `length` есть, а методов
интерфейса нет.

## Спека

DOM §4.2.10: `HTMLCollection` обязан иметь `length`, `item(index)` и
`namedItem(name)`; `NodeList` (DOM §4.2.10.1) — `length`, `item(index)`,
`entries()`/`keys()`/`values()`/`forEach()` и `Symbol.iterator`. Ни то, ни другое
не является `Array`, поэтому сайты зовут `item()`, а не `[]`.

## Что говорит измерение

Прогон 100 сайтов 2026-09-04: `a.item is not a function` — **первая** ошибка
консоли на `chatgpt.com` и `reddit.com`; на `airbnb.com` тот же дефект в трёх
формах (`document.getElementsByTagName(...).item`, `t.item`, плюс сопутствующие).
Три сайта разными библиотеками, то есть приём распространённый, а не экзотика.

## Класс дефекта

Тот же, что [BUG-715](BUG-715-FIXED.md) (`DOMTokenList`/`CSSStyleDeclaration`
собраны ad-hoc литералами вместо интерфейсов) и
[BUG-694](BUG-694-FIXED.md) (`URLSearchParams` без `Symbol.iterator`): объект
похож на нужный интерфейс по форме, но не по поведению. Эти два остаются
отдельными открытыми багами — не в объёме этого фикса.

## Объём (что проверялось)

Все точки, отдающие коллекцию: `getElementsByTagName`(`NS`),
`getElementsByClassName`, `getElementsByName`, `querySelectorAll`,
`children`, `childNodes` — на документе, элементе, `ShadowRoot`,
`DocumentFragment` и detached-документе (`DOMParser`/`createHTMLDocument`).
`document.images`/`forms`/`scripts`/`links` не трогались — это отдельный,
уже заведённый [BUG-892](BUG-892-OPEN.md) (`undefined`, а не «нет интерфейса»).

## Диагноз и фикс (P3, 2026-09-05)

`getElementsByName`/`children`/`document.images` уже были собраны через общую
Proxy-машину `_lumen_make_nid_collection` (даёт `item()`/`namedItem()`/индексы/
`length` за счёт `ownKeys`/`getOwnPropertyDescriptor` — сделано для BUG-310/
BUG-323/BUG-412). А вот `querySelectorAll`, `getElementsByTagName(NS)` и
`getElementsByClassName` — на документе, элементе, `ShadowRoot`,
`DocumentFragment` и detached-документе — были собраны в обход этой машины,
голым `.map(_lumen_make_element)` в голый JS `Array`: комментарии рядом честно
называли это «static array, not a live HTMLCollection», но не заметили, что
вместе с «не live» пропала и вся остальная часть интерфейса.

Фикс:
- `NodeList.prototype` получил `entries()`/`keys()`/`values()`/`Symbol.iterator`
  (был только `forEach`, `item` есть только на конкретных инстансах через Proxy) —
  все четыре построены на паре `length`+индексный доступ, которую любой Proxy
  этого файла уже отвечает.
- `_lumen_make_nid_collection` научился принимать необязательный `mapFn`
  (по умолчанию `_lumen_make_element`) — нужно было `document.childNodes`,
  которому нельзя терять kind-aware обёртку `_lumen_make_node` (BUG-321:
  doctype-ребёнок должен остаться `DocumentType`, а не элементом).
- Два тонких хелпера, `_lumen_static_node_list`/`_lumen_static_html_collection`,
  оборачивают уже посчитанный (статический, как и раньше) массив id в
  `NodeList`/`HTMLCollection` вместо голого массива — заменили голый `.map(...)`
  во ВСЕХ перечисленных в «Объём» точках; сама статичность (не live) не
  менялась, только интерфейс.
- `_lumen_collect_matching` (общий фильтр для `getElementsByTagName(NS)`) стал
  возвращать сырые id вместо уже обёрнутых элементов — обёртка теперь
  накладывается один раз, в вызывающем коде, через `_lumen_static_html_collection`.
- Попутно: `ShadowRoot.children` использовал `_lumen_get_children` (ВСЕ дети,
  включая текстовые узлы) вместо element-only `_lumen_make_html_collection`,
  которым уже пользуются `Element.children`/`DocumentFragment.children`
  (BUG-310) — выровнено с тем же хелпером.

**Ловушка при фиксе:** один существующий Rust-тест
(`v8_core::get_elements_by_tag_name_star_matches_elements_only`) звал `.map()`
прямо на результате `getElementsByTagName(...)` — рабочий приём, пока
результат был голым `Array`, но несовместимый с настоящим `HTMLCollection`
(та же несовместимость, что и в реальных браузерах — `HTMLCollection` не
имеет `.map`). Тест переписан на явный цикл по индексам; поведение,
которое он проверяет (порядок обхода, набор тегов), не изменилось.

**Проверено:** `cargo test -p lumen-js --features v8-backend` — 3465 passed,
0 новых падений (единственный красный — заранее известный [BUG-997](BUG-997-OPEN.md),
не относится к этой правке). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` — чисто. `scripts/scoped-test.sh` — зелёный (та же
единственная известная красная точка).

**Не сделано в этой правке (сознательно, отдельная работа):**
- `document.images`/`forms`/`scripts`/`links` (BUG-892) — не трогались, другой
  класс дефекта («нет вовсе», а не «нет интерфейса»).
- `MutationRecord.addedNodes`/`removedNodes` (`web_api_shim_mid_b.js`) по спеке
  тоже статический `NodeList`, а не голый массив — не входили в «Объём»
  исходной заявки, не трогались.
- Detached-документ (`DOMParser`/`createHTMLDocument`) хранит свой
  `_children` как JS-массив уже построенных wrapper-объектов, часть которых
  (detached `DocumentType`, см. `_lumen_make_detached_doctype`) не несёт
  `__nid__` вовсе — `.childNodes`/`.children` там остались голым массивом,
  переиспользовать nid-based Proxy без риска потерять `DocumentType`-обёртку
  не вышло за разумное время фикса.
- «Живое усиление» в исходной заявке (обвал `www.cnbc.com` через ~40-50с с
  `TypeError: object is not iterable` — предположительно тот же класс, что
  и здесь, но точный call site внутри бандла cnbc не был локализован) —
  повторный живой прогон `cnbc.com` для подтверждения/опровержения связи
  НЕ выполнялся в этой сессии (требует пересборки `lumen-shell` и живого
  GUI-окна; фикс собранного здесь класса дефектов корректен независимо от
  того, тот ли это самый call site). Если кто-то держит это в очереди — самый
  дешёвый способ проверить описан в исходной заявке (живой `--mcp-live-port
  --maximized`, опрос `resource://layout`/`resource://screenshot`).

## Сырые данные

`.tmp/perf-audit/20260904-150604/results.json` (slug `chatgpt`, `reddit`,
`airbnb`), `health.log`.
