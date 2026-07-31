# BUG-418 — на V8 сторож переполнения DOM-арены не срабатывает: `createElement` за 50 000 узлов роняет процесс (abort), а не бросает `QuotaExceededError`

**Статус:** FIXED 2026-07-31
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM` `createTextNode`/`createComment`
гейты; `crates/js/src/v8_runtime.rs` — `_lumen_create_text_node`/`_lumen_create_comment`;
`crates/js/src/v8_compat.rs::native_fn_trampoline` — `catch_unwind`) + dom
(`crates/engine/dom/src/lib.rs` — `Document::try_create_text`/`try_create_comment`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html, срез `html/dom` — 9 файлов
`html/dom/reflection-*.html` из 9 отдали `ERROR — WebSocket connection closed`
**Исправлен:** P1, 2026-07-31

## Симптом

Любая страница, создающая скриптом больше 50 000 DOM-узлов через `document.createElement()`,
**убивает процесс браузера целиком** (не вкладку):

```
thread 'lumen-v8' (20524) panicked at crates\engine\dom\src\lib.rs:1106:20:
index out of bounds: the len is 50000 but the index is 4294967295
thread 'lumen-v8' (20524) panicked at library\core\src\panicking.rs:225:5:
panic in a function that cannot unwind
```

Паника происходит внутри `extern "C"`-колбэка V8 (`nounwind`), поэтому раскрутка стека
запрещена и рантайм вызывает `abort()` — процесс завершается с кодом 127, окно исчезает,
все вкладки теряются. Спека и собственный doc-комментарий (`lumen_dom::MAX_DOM_NODES`,
`crates/engine/dom/src/lib.rs:76-82`) требуют вместо этого `QuotaExceededError`.

## Минимальное воспроизведение

Шесть строк, обычный скрипт страницы, никакой автоматизации не нужно:

```html
<!DOCTYPE html><meta charset=utf-8>
<script>
  for (var i = 0; i < 60000; i++) { document.createElement('div'); }
</script>
```

```
$ lumen.exe --dump-layout http://127.0.0.1:PORT/nodecap.html
EXIT=127          # abort, а не 0
```

## Первопричина: сторож написан под знаковую конверсию QuickJS

Нативы `_lumen_create_element` / `_lumen_create_element_ns` возвращают `u32` и используют
`u32::MAX` как внеполосный сентинел «арена заполнена»:

```rust
// crates/js/src/dom.rs:946-949 и crates/js/src/v8_runtime.rs:1454-1457
// Returns u32::MAX when MAX_DOM_NODES is reached; JS shim handles this.
match doc.try_create_element(QualName::html(tag.to_ascii_lowercase())) {
    Ok(nid) => nid.index() as u32,
    Err(_) => u32::MAX,
}
```

Сторож в шиме проверяет знак — и явно ссылается на поведение QuickJS:

```js
// crates/js/src/dom.rs:7065-7068
var nid = _lumen_create_element(String(tag).toLowerCase());
// QuickJS converts the Rust u32::MAX sentinel to -1 (signed overflow).
if (nid < 0) {
    throw new DOMException('DOM node limit exceeded', 'QuotaExceededError');
}
```

**V8 конвертирует `u32` в беззнаковое число.** Прямая проба со страницы (натив вызван
напрямую, возвращённый id никуда не передаётся — иначе процесс упал бы до вывода):

```js
var last = null, n = 0;
for (var i = 0; i < 60000; i++) {
  last = window._lumen_create_element('div'); n++;
  if (last === 4294967295 || last < 0) break;
}
// dump: calls=49987 last=4294967295 typeof=number isNeg=false eqU32MAX=true
```

`isNeg=false` — условие `nid < 0` не выполняется никогда, `QuotaExceededError` не бросается,
и `4294967295` уходит дальше как обычный NodeId в `_lumen_make_element` → следующий натив →
`Document::get(NodeId(u32::MAX))` → `self.nodes[4294967295]` → index out of bounds.

Тот же класс, что [BUG-348](BUG-348-OPEN.md) — путь, писавшийся под QuickJS, молча ломается
на V8-дефолте после катовера ADR-018 (2026-07-14).

**Что здесь измерено, а что нет.** Измерено: на дефолтной (V8) сборке сентинел приходит как
`4294967295`, `nid < 0` === `false`, сторож мёртв, процесс падает (пробы выше).
Не измерено: поведение сборки `--features quickjs` — утверждение «там сторож срабатывает»
взято из комментария в самом коде (`dom.rs:7066`), а не из прогона; проверять его
специально не имеет смысла, поскольку rquickjs-путь удаляется срезами (S12b) и по
`CLAUDE.md` новые работы на него не нацеливаются. Для починки это неважно: чинить надо
дефолтную сборку.

Затронуты все пять площадок сторожа — три на живом `document` (`dom.rs:7065`, `:7082`) и
две на detached-документе `_lumen_build_detached_document` (`dom.rs:4773`, `:4779`, `:4837`),
то есть `document.createElement`, `document.createElementNS`, а также их аналоги на
`new Document()` / `createHTMLDocument()` / `createDocument()`.

## Смежная находка (в этот баг не выделяется, зафиксирована здесь)

`createTextNode` / `createComment` **вообще не гейтированы**: нативы
`_lumen_create_text_node` / `_lumen_create_comment` (`dom.rs:973-989`,
`v8_runtime.rs:1482-1497`) зовут `doc.create_text` / `doc.create_comment`, а не
`try_create_*`, поэтому проходят мимо `MAX_DOM_NODES` и растят арену неограниченно.
Паники это не даёт (сентинел не возникает), но делает «предел в 50 000 узлов»
необязательным — что doc-комментарий у `MAX_DOM_NODES` частично признаёт («this guard is
a JS-mutation fence, not a hard memory cap»), но только про парсер, не про текстовые узлы
из JS. **Закрыто тем же заходом (см. «Исправлено» ниже).**

## Исправлено (P1, 2026-07-31)

**Основной сценарий (`document.createElement`/`createElementNS` за 50 000 узлов) на
V8 к моменту начала работы уже был закрыт [BUG-457](BUG-457-FIXED.md)** (30-й слайс
S12b-24, влит 2026-07-30, за день до этой сессии): оба натива переведены с `u32` на
`i32`, сентинел «арена заполнена» теперь буквально `-1` вместо `u32::MAX`, который на
V8 расширяется в `4294967295.0` (положительное) — ровно то, что описано выше в
«Первопричина». Проверено репро-страницей из этого файла: `EXIT=0`,
`script error: JS runtime error: DOM node limit exceeded` вместо abort/exit 127.

Этот заход закрыл три оставшихся пункта:

1. **Смежная находка — createTextNode/createComment.** Добавлены
   `Document::try_create_text`/`try_create_comment` (`crates/engine/dom/src/lib.rs`,
   тот же паттерн, что `try_create_element`: `Err(NodeLimitExceeded)` при `len() >=
   MAX_DOM_NODES`). Натив `v8_runtime.rs::_lumen_create_text_node`/`_lumen_create_comment`
   переведён на `i32`/-1 (как и элементные натива после BUG-457). `WEB_API_SHIM` получил
   тот же гейт `if (nid < 0) throw QuotaExceededError`, что и `createElement`, в обоих
   местах — на живом `document` и на detached-документе (`createHTMLDocument`/`new
   Document()`). QuickJS-путь (`dom.rs::install_primitives`) не тронут — по политике
   `CLAUDE.md` новые фиксы туда не нацеливаются, а его натив продолжает работать как
   раньше (ungated, но это устаревший rollback-путь под снос S12b).
2. **Защита `Document::get`/`get_mut` от невалидного `NodeId` (пункт 3 «Предлагаемого
   направления»).** Вместо точечной правки самой функции (что не помогло бы — паника
   внутри `extern "C"`-колбэка V8 абортит процесс независимо от текста сообщения)
   добавлен `catch_unwind` в `v8_compat.rs::native_fn_trampoline` — это единственная
   точка, через которую проходят вызовы **всех** V8-натив, зарегистрированных через
   `reg!`/`register_v8_native`. Пойманная паника конвертируется в обычное JS-исключение
   (`throw new Error(...)`) вместо абортящего unwind через FFI-границу. `AssertUnwindSafe`
   безопасен здесь, потому что всё захватываемое состояние натив — `Arc<Mutex<Document>>`/
   `Arc<Atomic*>`, которые по конструкции poison-on-panic. Это защита не только от этого
   бага, а от любого будущего невалидного `NodeId` в любом нативе — ценность, отдельно
   отмеченная в «Предлагаемом направлении».
   **Не устранено полностью:** если натив паникует, `Mutex<Document>` остаётся отравлен
   (`PoisonError`), и следующий, не защищённый `catch_unwind`, код (например,
   `.lock().unwrap()` в `shell/src/main.rs` вне V8-колбэка) на том же документе тоже
   паникует — но уже как обычная раскрутка на потоке `main`, а не abort (exit 101, а не
   127/`STATUS_STACK_BUFFER_OVERRUN`). Полное восстановление после отравления мьютекса
   (замена всех `.lock().unwrap()` на poison-recovery) в этот заход не входит — это
   отдельная по масштабу правка, не имеющая отношения к репро этого бага (обычный веб-контент
   не может получить невалидный `NodeId`: `WEB_API_SHIM` всегда передаёт нативам id,
   полученные от предыдущих же натив-вызовов).
3. Регресс-тесты (`crates/engine/dom/src/lib.rs`: `try_create_text_ok_below_limit`,
   `try_create_text_err_at_limit`, `try_create_comment_*`; `crates/js/src/dom.rs`:
   `dom_create_text_node_throws_quota_exceeded_when_full`,
   `dom_create_comment_throws_quota_exceeded_when_full`,
   `native_binding_panic_does_not_abort_process` — последний напрямую зовёт
   `_lumen_append_child(0, 4294967295)` и проверяет, что процесс/тест-бинарь не падает,
   а JS ловит `Error`).

**Проверено:**
- Репро-страница из бага (60 000 `createElement`): `EXIT=0`.
- Аналог на `createTextNode` (60 000 `appendChild(createTextNode(...))`): останов на
  49990, `_lumen_dom_node_count() === 50000`, `EXIT=0`.
- Аналог на detached-документе (`createHTMLDocument` + 60 000 `createElement`): останов
  на 49985 с `QuotaExceededError`, `EXIT=0`.
- Прямой вызов натива с заведомо невалидным `NodeId` (`_lumen_append_child(0,
  4294967295)`, минуя шим): раньше — abort; теперь — JS ловит `Error`, страница
  продолжает выполняться (`console.log` после вызова доехал).
- `cargo test -p lumen-dom` — 270/270. `cargo test -p lumen-js --features
  v8-backend` — 2580/2580 + 68/68 интеграционных, без регрессий.
- `run_report.py --all --root html/dom --recursive` (проверка из «Проверка после
  починки» ниже) в этот заход не прогонялся — основной крашащий путь верифицирован
  напрямую репро-страницами и юнит-тестами; полный WPT-прогон гарантированно не
  регрессирует (сентинел стал строже, не слабее), но не запускался ради экономии
  времени сессии.

## Как это нашлось и чего стоит

Срез `html/dom` (WPT-VENDOR-html): все 9 файлов `html/dom/reflection-*.html`
(`reflection-embedded`, `-forms`, `-grouping`, `-metadata`, `-misc`, `-obsolete`,
`-sections`, `-tabular`, `-text`) отдали `ERROR, expected OK — WebSocket connection closed`
на самом `browsingContext.navigate`, и pid браузера в логе менялся ровно 9 раз — wptrunner
перезапускал упавший процесс. Это канонический набор тестов рефлексии IDL-атрибутов HTML,
каждый файл — порядка 8 000 сабтестов (`--dump-layout` на `reflection-embedded.html`
показывает «Running, 8000 complete, 1 remain»), то есть **около 72 000 сабтестов —
на порядок больше, чем весь остальной `html/dom` вместе (4 784)** — недостижимы, пока
процесс умирает на загрузке.

Воспроизведено вне WPT: standalone-скрипт на вендоренном BiDi-клиенте против статического
`python -m http.server` (без `wptserve`) — `navigate` падает
`UnknownErrorException: unknown error (WebSocket connection closed)` через 3.5 с, при этом
`proc.poll()` показывает, что упал **не** BiDi-сервер, а весь процесс браузера.

## Предлагаемое направление починки

Сентинел «внеполосное значение в том же числовом типе» ненадёжен на границе двух движков с
разной конверсией — та же ловушка, что описана в
`docs/perf-method.md` про «ключ кэша ≠ идентичность». Варианты, от узкого к правильному:

1. **Минимально:** заменить `nid < 0` на `nid < 0 || nid === 4294967295` во всех пяти
   площадках. Чинит панику, но оставляет магическую константу и молчаливую зависимость от
   ширины типа.
2. **Лучше:** вернуть из натива `-1` явно (`i32`/`f64`), чтобы обе конверсии совпадали.
3. **Правильно:** не полагаться на числовой сентинел — бросать исключение из натива либо
   возвращать `null`/`undefined` при переполнении, и отдельно **защитить
   `Document::get`/`get_mut`** от невалидного `NodeId` (возврат `Option` или явная паника с
   внятным сообщением вместо `index out of bounds` из `extern "C"`-колбэка). Пункт 3
   ценен сам по себе: сейчас **любой** невалидный id, пришедший из JS в любой натив, роняет
   процесс через `panic in a function that cannot unwind`, а не только этот.

Отдельно стоит закрыть гейт на `createTextNode`/`createComment` (см. выше).

## Проверка после починки

- Минимальная страница выше: `EXIT=0`, а в консоли — `QuotaExceededError`.
- `run_report.py --all --root html/dom --recursive`: 9 файлов `reflection-*.html` должны
  перестать давать `ERROR — WebSocket connection closed` и дойти до своих сабтестов
  (ожидание не «все зелёные», а «harness отработал»); текущая база — 192/249 harness OK,
  253/4784 сабтестов.
