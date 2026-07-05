# Задача: structuredClone + Transferable objects

**Developer:** P1
**Ветка:** `p1-structured-clone`
**Размер:** M
**Крейты:** `lumen-js`

## Goal
Довести `structuredClone(value, { transfer })` до соответствия алгоритму
StructuredSerialize/Deserialize (HTML LS §2.7): добавить типизированные массивы,
`ArrayBuffer`/`DataView`, `Blob`/`File`, `Error`, циклические ссылки; и поддержать
Transferable — перенос `ArrayBuffer` (и по возможности `ImageBitmap`/
`OffscreenCanvas`) с обнулением источника (`detached`).

## Current state (сверено с кодом 2026-07-05)
- Единственная реализация — JS-функция `structuredClone` в
  `crates/js/src/dom.rs:11454-11480`.
- Комментарий-контракт `dom.rs:11451-11453` прямо перечисляет **что не
  обрабатывается**: typed arrays как значения, циклические ссылки, функции,
  символы.
- Поддержано: примитивы (:11455-11457), `Date` (:11458), `RegExp` (:11459),
  `Map` (:11460), `Set` (:11465), `Array` (:11470), plain-object (:11475).
- Экспорт — `window.structuredClone = structuredClone` (`dom.rs:11480`).
- **Второй аргумент `{ transfer }` полностью игнорируется** — сигнатура
  `function structuredClone(val)` принимает один параметр.
- Рекурсия делает глубокую копию БЕЗ таблицы memory → цикл `a.self = a`
  уйдёт в бесконечную рекурсию (переполнение стека), а два ссылки на один объект
  дают две копии (нарушение identity-инварианта алгоритма).
- Тесты — `dom.rs:20804+` (`structuredClone(42) === 42` и т.п., базовые).
- ArrayBuffer transfer/detach: поиск по `detached`/`transfer` в `crates/js/src`
  совпадений в контексте structuredClone не даёт — фичи нет.

## Entry points
- `crates/js/src/dom.rs:11454` — тело `structuredClone` (переписать с таблицей
  memory и разбором `transfer`).
- `crates/js/src/dom.rs:11451` — комментарий-контракт (обновить перечень).
- `crates/js/src/dom.rs:11480` — точка экспорта (сигнатура с двумя аргументами).
- Тесты рядом с `dom.rs:20804` (модуль тестов рантайма).

## Срезы (декомпозиция)
### Срез 1 — S — memory-таблица (identity + циклы)
Ввести внутренний `_structuredCloneImpl(val, memory)` где `memory` — `Map`
исходный→клон. Перед клонированием объекта проверять `memory.has(val)`; после
создания клон-контейнера сразу класть в `memory`, затем заполнять. Убирает
бесконечную рекурсию на циклах и сохраняет shared-identity (две ссылки → один
клон). Обновить комментарий `dom.rs:11451`.

### Срез 2 — S — TypedArray / ArrayBuffer / DataView (копирование)
Ветки для `ArrayBuffer` (`val.slice(0)`), всех `%TypedArray%`
(`Int8Array…Float64Array`, `BigInt64/Uint64`) и `DataView` — создать новый view
над клонированным буфером. Учесть общий буфер: если два typed-array смотрят в
один ArrayBuffer, через memory они должны делить один клон-буфер.

### Срез 3 — S — Error / Blob / File / прочие platform-объекты
`Error` и подклассы (сохранить `name`/`message`/`stack`), `Blob`/`File`
(если типы определены в шиме — проверить наличие через `typeof Blob`).
Неклонируемое (функции, symbol-значения, DOM-узлы вне списка) →
`DataCloneError` (`DOMException`), как требует спек.

### Срез 4 — S — второй аргумент { transfer } + ArrayBuffer detach
Расширить сигнатуру до `structuredClone(val, options)`. Разобрать
`options.transfer` (массив Transferable). Для каждого `ArrayBuffer` из списка:
перенести (не копировать) содержимое в результат и **отсоединить источник** —
пометить `detached`, обнулить `byteLength`, бросать при последующем доступе.
Проверить, доступен ли нативный detach через rquickjs; если нет — эмулировать
флагом `__detached` на объекте-обёртке (описать ограничение в комментарии).

### Срез 5 — S — ImageBitmap / OffscreenCanvas transfer (по возможности)
Проверить существование типов: `offscreen_canvas.rs`, `img_bitmap_store.rs`
уже есть в `crates/js/src/`. Если у них есть transfer-совместимое
представление — реализовать перенос; иначе задокументировать как отложенное и
бросать `DataCloneError` при попытке transfer этих типов (спек-корректно лучше,
чем молчаливая копия).

## Tests
- JS-интеграция в `dom.rs` (рядом с `:20804`): цикл `var a={}; a.self=a;
  var c=structuredClone(a); c.self===c`; shared identity
  (`{x:o, y:o}` → `c.x===c.y`); typed array (`Int32Array` значения совпадают,
  буфер — новый); `ArrayBuffer` transfer: клон получает данные, источник
  `.byteLength===0`/detached; неклонируемое (функция) → бросает `DataCloneError`.
- Регрессия: существующие тесты Map/Set/Date/RegExp (`:20804+`) остаются зелёными.

## Definition of done
- [ ] memory-таблица: циклы не падают, shared identity сохранён.
- [ ] TypedArray/ArrayBuffer/DataView клонируются (общий буфер — один клон).
- [ ] Error и (при наличии типов) Blob/File клонируются.
- [ ] Неклонируемое → `DataCloneError`.
- [ ] `{ transfer }` переносит ArrayBuffer с detach источника.
- [ ] ImageBitmap/OffscreenCanvas: transfer или спек-корректный отказ.
- [ ] Комментарий-контракт `dom.rs:11451` актуализирован.
- [ ] `CAPABILITIES.md` — structuredClone 🟡 → ✅.
