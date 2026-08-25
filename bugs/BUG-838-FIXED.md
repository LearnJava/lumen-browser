# BUG-838 — `<script src="">` (пустой `src`) не даёт ни `error`, ни `load`: элемент молча трактуется как пустой инлайн-скрипт

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/dom.rs:5844`–`5849` (`_lumen_script_prepare`: пустой `src` после `trim()` уходит в ветку инлайн-тела и выходит по `return`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var s = document.createElement("script");
s.onerror = () => console.log("error");   // не приходит
s.onload  = () => console.log("load");    // тоже не приходит
s.setAttribute("src", "");
document.body.appendChild(s);
```

Ни события, ни сообщения в stderr, ни сетевого запроса. Страница, которая
ждёт `error` (а это ровно то, что предписывает спека), висит.

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py --variant script-src-empty`
(2026-08-22, dev-release, Linux, коммит `762a0cad9`, `--seconds 6`,
страница жива — 11 тиков):

| ожидалось | получено |
|---|---|
| `script-error type=error` | `appended-empty-src` — и всё |

## Причина (локализована чтением кода)

```js
var src = _lumen_u2n(_lumen_get_attr(nid, 'src'));
src = (src === null) ? '' : String(src).trim();          // dom.rs:5845
if (src !== '') { _lumen_script_load_external(nid, src, isModule); return; }
var body = _lumen_u2n(_lumen_get_text_content(nid));
if (body === null || String(body).trim() === '') return;  // dom.rs:5849 — тихий выход
```

Отсутствующий и пустой `src` сведены в одно значение `''`, после чего
элемент идёт по инлайн-ветке и выходит без событий, потому что тела нет.
HTML LS §4.12.1 «prepare the script» различает эти случаи: если атрибут
`src` присутствует и его значение — пустая строка, шаги требуют
**поставить задачу, которая выстрелит `error`** (то же самое, что при
неудачной загрузке).

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: на этой форме в остатке
WPT-RUN-5 стоят два id —
`html/semantics/scripting-1/the-script-element/fetch-src/empty.html` и
`empty-with-base.html`, — и обоих мало для отдельного правила, которое
надёжно отделяло бы `src=""` от обычного внешнего скрипта по исходнику.
Заводится по прямому замеру, как [BUG-825](BUG-825-FIXED.md).

Родственный (но отдельный) дефект — [BUG-804](BUG-804-OPEN.md): парсерная
вставка `<script src>` вообще не доставляет `load`. Здесь речь о другом
входе (элемент создан скриптом) и о другом событии.

## Направление починки (не предписание)

Разделить «атрибута нет» и «атрибут пустой»: сохранить результат
`_lumen_get_attr` до нормализации и, если атрибут присутствует, но пуст,
вызвать `_lumen_resource_fire(nid, 'error')` через `setTimeout(…, 0)` (тем
же способом, каким уже отправляется `error` при провале внешней загрузки,
`dom.rs:5905`).

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant script-src-empty` — ожидается `script-error type=error`.
2. WPT: `run_report.py --all --root html/semantics/scripting-1/the-script-element/fetch-src --recursive`.

---

## Починено (P1, 2026-08-25)

Заявка называла один вход — элемент, созданный скриптом. Тем же дефектом
страдал **второй, соседний путь**: `<script src="">`, написанный парсером,
до JS-шима вообще не доходит, а сборщик шелла
`main.rs::collect_scripts_ordered:7293`–`7298` роняет пустой `src` ровно так
же молча (`if !src.is_empty()` … `return`). Поэтому починка состоит из двух
половин, а не из одной.

**JS-половина** (`crates/js/src/dom.rs`). Разделены «атрибута нет» и «атрибут
пуст»: `_lumen_script_has_empty_src(nid, isModule, type)` отвечает по
*наличию* атрибута, а не по нормализованному значению, и
`_lumen_script_fire_empty_src_error` ставит задачу через `setTimeout(…, 0)` —
тем же прыжком, что и путь внешней загрузки рядом. Задача, а не синхронная
доставка: обработчик вешается ДО `appendChild`, но `empty.html` отдельным
ассертом требует, чтобы событие не пришло синхронно (`assert_true(queued)`).

**Парсерная половина.** Правка ушла не в шелл, а в тот же шим: `load`/`error`
принадлежат элементу, и порядок «сначала весь разбор, потом скрипты» не даёт
шеллу точки, где такой элемент был бы вставлен (тот же довод, по которому в
шиме живут `rel=stylesheet` — BUG-703 — и подсказки `rel=preload` —
BUG-826). Проход `_lumen_script_empty_src_scan()` вызывается из
`_lumen_apply_ready_state('interactive')` рядом с `_lumen_link_hints_scan()`,
по той же форме. Пер-узловой флаг `_lumen_script_empty_src_done` держит два
входа на одном событии: элемент, который head-скрипт создал и вставил, уже
отчитался через хук вставки и повторно проходом не берётся.

**Третий дефект, которого заявка не называла**, — само событие. WPT-тесты
проверяют не только факт `error`, и на них видно, что `_lumen_resource_fire`
строил событие без `isTrusted` и без `target` (`_lumen_dispatch` не проставляет
`target` вообще — BUG-873). Событие, порождённое движком, доверенное по
определению, а обработчик без `ev.target` не отличит два висящих скрипта друг
от друга, поэтому оба поля теперь заполняются — на весь резольверный путь
`load`/`error` разом, не только на пустой `src`.

Пробел `src="   "` трактуется как пустой: буквальное чтение спеки отправило бы
его в разбор URL, а тот срезает ровно этот пробел и резолвит ссылку в сам
документ — скачать собственный HTML, чтобы не разобрать его как JS, значит
прийти к тому же `error`, только с лишним запросом.

### Замеры

| проверка | до | после |
|---|---|---|
| `verify_navigation_form_import_gaps.py --variant script-src-empty` | `appended-empty-src` — и всё | `appended-empty-src`, `script-error type=error` |
| весь набор проб (28 вариантов) | — | все контрольные целы, `script-change-src-attr` по-прежнему просит у сервера только первый URL |
| `fetch-src/empty.html`, `empty-with-base.html` | TIMEOUT | FAIL на последнем ассерте (`assert_class_string`), 4/4 harness OK |
| `fetch-src/failure.html` | FAIL на `isTrusted` | FAIL на `assert_class_string` |
| `cargo test -p lumen-js --features v8-backend bug838` | — | 4/4 |

Тесты: `bug838_empty_src_script_fires_error_as_a_task` (тип/`bubbles`/
`cancelable`/`isTrusted`/`target` + асинхронность),
`bug838_empty_src_script_reports_once_across_both_paths` (дедупликация двух
входов), `bug838_empty_src_script_from_markup_fires_error` (парсерный элемент
строится в арене — `innerHTML` `<script>` не просто инертен, он не создаётся
вовсе), `bug838_absent_src_and_empty_body_stays_silent` (контроль: ветка «нет
атрибута» осталась молчаливой).

### Остаток

Три id категории теперь падают на одном и том же последнем ассерте —
`assert_class_string(ev, "Event")`: `Object.prototype.toString` даёт
`[object Object]`, потому что `Symbol.toStringTag` есть ровно у одного из 26
конструкторов событий шима (`ErrorEvent`, заведён попутно с BUG-591/813).
Это общий дефект иерархии `Event`, а не свойство пустого `src` (на нём же
стоит и `failure.html`, к этому багу отношения не имеющий) — вынесен в
[BUG-912](BUG-912-OPEN.md).

### Поглощённый дубль: BUG-853

[BUG-853](BUG-853-DUPLICATE.md) (заведён 2026-08-22, WPT-RUN-6 срез 24) —
та же ветка `_lumen_script_prepare`, та же пара id, тот же шаг спеки; разошлись
только номера строк, потому что `dom.rs` сдвигался между срезами
(`:5844` здесь против `:6533` там). Замер среза 24 покрывал лишь вход
«создан скриптом», то есть подмножество этой заявки.

Перезамер его пробой уже после починки (P1, 2026-08-25, `main` = `49046b7dc`,
dev-release, Windows, `--seconds 6`, страница жива — 4 тика):
`verify_frame_load_media_gaps.py --variant script-empty-src` печатает
`script-empty-error` (было — тишина) при целом контроле `script-good-load
ran=1`, и сервер пробы по-прежнему не видит запроса с пустым путём. Это второй,
независимый от `verify_navigation_form_import_gaps.py` вход в тот же дефект.

Что дубль добавил поверх этого файла и что поэтому переехало сюда: маркер
`script-empty-src` в `tests/wpt/timeout_audit.py` (его `ref` переведён на
BUG-838) и явные имена двух id —
`html/semantics/scripting-1/the-script-element/fetch-src/empty.html` и
`empty-with-base.html`, оба `async_test`, ждущие `error`. Раздел «Масштаб»
выше объясняет, почему сам этот файл маркера не заводил; теперь маркер есть,
и владелец у него один.
