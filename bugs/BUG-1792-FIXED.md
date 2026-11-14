# BUG-1792 — lenta.ru: `Uncaught TypeError: e.children is not iterable` в коде плеера/виджетов

**Статус:** FIXED 2026-11-14 (P3)
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Область:** js — `HTMLCollection.prototype` без итератора

## Причина

`NodeList.prototype` имел `Symbol.iterator`/`entries`/`keys`/`values` (BUG-994), а `HTMLCollection.prototype` — нет. Геттер `children` всех интерфейсов (`_LUMEN_WRAPPER_MEMBERS` для элементов — HTML/SVG/MathML/shadow-хосты, `ShadowRoot.prototype`, `DocumentFragment`/`template.content`, живой `Document`) отдаёт живую коллекцию через `_lumen_make_html_collection` — Proxy поверх прототипа `HTMLCollection`, у которого `@@iterator` отсутствовал, поэтому `[...el.children]`/`for..of` бросали `TypeError: e.children is not iterable` (минимизированный код плеера lenta.ru). Гипотеза карточки про отдельный путь сборки коллекции для SVG/MathML/shadow root не подтвердилась — путь один, итератора не было у прототипа. HTML-спека интерфейс итерируемым не объявляет, но все три движка делают его итерируемым (Blink/WebKit — `iterable<Element>` в IDL, Gecko — с FF 51): спред по `children` — де-факто платформенное поведение.

## Фикс

`crates/js/src/shim/web_api_shim_mid.js`: на `HTMLCollection.prototype` определены `forEach`/`entries`/`keys`/`values`/`@@iterator` поверх существующего `_lumen_index_iterator` (тот же length+index-механизм, что у NodeList). Все члены — non-enumerable (`Object.defineProperty`), как у WebIDL-членов интерфейса: обычное присваивание `proto.m = fn` делает метод enumerable, он протекает в `for-in` по коллекции и ломает контракт BUG-323 («в `for-in` по `HTMLCollection` — только индексы») — это сразу поймал регрессионный `html_collection_supports_enumeration`, после чего определения переведены на `defineProperty`.

Попутно починен гейт, не давший закрыть задачу: 28 модулей `crates/js/src/dom/tests/mod.rs` (`v8_*` + `compat_css_alias`) были объявлены без `#[cfg(feature = "v8-backend")]` и ломали компиляцию `cargo test -p lumen-js --tests` и `cargo clippy --workspace --all-targets` без фичи (resolver 3 не унифицирует фичи в lib-test-юнит; красный с мержей BUG-1553/UX-SELECTION и cssom-пачки) — расставлены по конвенции файла.

## Проверка

- Новые интеграционные тесты `crates/js/tests/cases/bug1792_html_collection_iterable.rs` (3): спред/`for..of`/`Array.from` по `children` HTML-, SVG-, shadow-host-элементов; `ShadowRoot`/`template.content`/`Document`/статическая `getElementsByTagName`-коллекция; протокол итератора (`values`/`keys`/`entries`/`forEach`/`@@iterator`) и non-enumerable членов (индексы-only в `for-in`, BUG-323).
- Живой прогон compat-crawl lenta (`--only lenta`, dev-release с фиксом, Chrome for Testing 153, 2026-11-14): `PARITY lumen=OK`, exceptions-трек зелёный (было стабильное `e.children is not iterable`), CPR 100 % (`.tmp/compat-crawl/20261114-135101/`, не коммитится).
- Гейт: `cargo clippy --workspace --all-targets -- -D warnings` зелёный; scoped-прогон — падают только задокументированные чужие красные (BUG-1809, BUG-1236, BUG-1795, flaky-трио BUG-1677) плюс `lumen-paint`/`lumen-media-native`-тесты, воспроизведённые на чистом main-checkout вне этого бранча.

## Связи

- NodeList-итераторы — BUG-994; собственные ключи коллекции — BUG-323.
- Compat-crawl отчёт (исходный симптом): `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`).
