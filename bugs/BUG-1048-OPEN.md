# BUG-1048 — Скриптом созданный/переприсвоенный `<img>` (BUG-730 «streaming/dynamic» путь) фетчится и декодируется, но не диспатчит `load`/`error` и не обновляет `complete`/`naturalWidth`/`naturalHeight`

**Статус:** OPEN
**Область:** `crates/shell/src/app/user_event.rs:51-87` (`LoadEvent::ImageDecoded` — только рендерер + `stream_image_sizes`, нет события), `crates/shell/src/page_load.rs:1015-1080` (`spawn_dynamic_image_loads`/`apply_stream_intrinsic_sizes` — URL-keyed, без per-node dedup, без канала ошибки), `crates/shell/src/page_load.rs:1102-1140` (`spawn_image_requests` — decode failure — `None` arm — молча ничего не шлёт, ни лога, ни события)
**Найден:** P1, GAP-LOADEV срез 1 (BUG-630), живой замер `verify_callback_import_preload_gaps.py --variant img-onload-attr`, 2026-09-12

## Симптом

После починки BUG-630 (парсерный `<img>` диспатчит `load`/`error` через
`page_pipeline.rs`'s eager pipeline) тот же live-замер показал асимметрию:
`<img>`, написанный ПАРСЕРОМ, диспатчит нормально
(`ioa-parser-listener-fired`, `ioa-parser-attr-fired`). `<img>`, созданный
скриптом (`document.createElement('img')`, `.src = url`, `appendChild`)
ПОСЛЕ завершения первичной загрузки (внутри `window.onload`) — сервер пробы
ВИДИТ запрос (`GET /vcip-pixel.png?script-made`, то есть фетч реально
произошёл — это НЕ BUG-1048-класс «фетча не было вовсе»), но ни
`ioa-script-attr-fired`, ни `ioa-script-listener-fired` не печатаются:
`img.complete` для этого узла остаётся `false` навсегда.

Второй репрод (`verify_window_history_jsurl_gaps.py --variant canvas-misc`,
уже описан в BUG-630 «Перезамер, срез 28»): `new Image()`, никогда не
вставленный в документ, — та же немота, и здесь `server saw: nothing` (0
запросов вообще: `collect_image_requests` обходит DOM, а неприкреплённый
узел не виден ни на одном из трёх проходов, включая этот).

## Причина (уточнена чтением кода — это НЕ «фетча вовсе нет»)

BUG-730 УЖЕ решил ровно эту задачу на уровне байтов: `spawn_dynamic_image_loads`
(`page_load.rs:1015`) гоняется после каждого relayout с DOM-мутацией, находит
через `collect_image_requests` картинки, которых ещё не было в
`stream_images_requested`, и фетчит+декодирует их тем же
`spawn_image_requests` (`page_load.rs:1102`), которым streaming-пролог
грузит партиальный DOM. Результат приходит `LoadEvent::ImageDecoded`
(`page_load.rs:1791`) — тот же тип события, что и streaming-путь.

Разрыв в двух местах:

1. **`user_event.rs:51` (`ImageDecoded`-обработчик)** регистрирует пиксели в
   рендерере и копит `(url, w, h)` в `self.stream_image_sizes` для
   ОТЛОЖЕННОГО прохода `apply_stream_intrinsic_sizes` (BUG-735) — ни там, ни
   там нет вызова `fire_image_load`/`fire_image_error` (эти два метода
   появились только в GAP-LOADEV срезе 1, уже ПОСЛЕ BUG-735).
2. **Событие ключуется по `url`, не по `node_id`** — в отличие от eager-пайплайна
   (`page_pipeline.rs`, где `ImageRequest.node_id` известен в момент декода),
   здесь несколько `<img>` могут делить один URL, а сам узел мог появиться в
   DOM уже ПОСЛЕ того, как декод для этого URL завершился (дедуп по
   `stream_images_requested` намеренно не перезапрашивает). Разнос
   `url -> nodes` делает только `apply_stream_intrinsic_sizes`
   (`page_load.rs:1042`), коалесцированным проходом раз в кадр — и то только
   для intrinsic-size, без понятия «этому узлу уже сообщили `load`».
3. **У decode-неудачи нет события вовсе.** `spawn_image_requests`
   (`page_load.rs:1123-1128`), арм `None`: комментарий "streaming best-effort:
   финальный pipeline залогирует/применит" — для СКРИПТОВОГО `<img>` после
   `LoadDone` никакого «финального pipeline» уже не будет, ошибка
   растворяется молча, `img.onerror` не сработает никогда.
4. **Неприкреплённый `new Image()`** не виден вообще ни одному из трёх
   проходов (`collect_image_requests` обходит только связное DOM-дерево) —
   отдельная, более узкая грань той же причины: спека фетчит по смене `src`,
   а не по присоединению к дереву (HTML LS §4.8.4.2 «update the image data»).

## Масштаб

Любой клиентски отрисованный код, добавляющий/меняющий `<img>` после
первичной загрузки — то есть весь client-side-rendered веб (карточка BUG-730
уже цитирует `tbank.ru`: ни одна из 33 картинок не была частью первичного
HTML). Байты долетают и рисуются, но `<img>.onload`/`.complete`/
`naturalWidth`/`naturalHeight` для скрипта, ĸоторый строит layout уже
ПОСЛЕ появления картинки (частый паттерн галерей/каруселей), не работают
никогда.

## Что дальше (не предписание)

Нужен per-node dedup («этому nid уже сообщили load/error для этого
поколения decode»), например множество `HashSet<u32>` рядом с
`stream_image_sizes`, взводимое в `apply_stream_intrinsic_sizes`'s проходе
(он уже сопоставляет `url -> node_id` через свежий `collect_image_requests`)
— и новый вариант `LoadEvent` (или расширение `ImageDecoded`) для
decode-неудачи, которого сейчас нет вовсе. Неприкреплённый `new Image()` —
отдельная, более глубокая правка (нужен hook на присвоение `.src`, а не на
DOM-обход), вне бюджета минимального фикса этой карточки.

## Как проверить фикс

1. `verify_callback_import_preload_gaps.py --variant img-onload-attr` —
   `ioa-script-attr-fired`/`ioa-script-listener-fired` начинают печататься.
2. `verify_window_history_jsurl_gaps.py --variant canvas-misc` (после
   правки на прикреплённый `<img>`; неприкреплённый `new Image()` остаётся
   отдельным пунктом) — `drew-svg`/`toDataURL` печатаются.

## Срез 1 (GAP-LOADEV, 2026-09-13, `p1-gap-loadev-bug1048`) — прикреплённый скриптовый `<img>` теперь диспатчит `load`/`error`

Закрыты пункты 1–3 причины (выше) для прикреплённого узла; пункт 4
(неприкреплённый `new Image()`) не тронут — отдельная, более глубокая правка
(нужен hook на присвоение `.src`, а не на DOM-обход), вне бюджета.

* **Новое событие для decode-неудачи.** `LoadEvent::ImageDecodeFailed { src }`
  (`page_load.rs`) — `spawn_image_requests`'s `None`-арм (`page_load.rs:1134`)
  раньше молча ничего не слал; теперь шлёт этот вариант тем же `proxy`, что
  уже шлёт `ImageDecoded`. Обработчик в `user_event.rs` кладёт `src` в новое
  поле `stream_image_errors` (зеркало `stream_image_sizes` для неудачи) и
  взводит тот же `stream_image_sizes_dirty`, каким уже коалесцируется путь
  успеха — включая явный `window.request_redraw()` (без него флаг лежал бы
  непрочитанным до случайного соседнего перерисовывания: первая версия среза
  забыла его и `img-onerror-dynamic`-проба ничего не печатала, хотя
  `stream_image_errors` уже содержал URL).
* **Per-node dedup.** Новое поле `stream_image_events_fired: HashSet<(u32,
  String)>` (пара node-index + url) — `apply_stream_intrinsic_sizes`
  (`page_load.rs:1044`), тот же коалесцированный проход, что уже сопоставляет
  `url -> node_id` для intrinsic-size, теперь на каждое совпадение (успех ИЛИ
  зафиксированная неудача) при первом попадании в `stream_image_events_fired`
  зовёт `fire_image_load`/`fire_image_error` через `route_task_js` — без
  дедупа один и тот же узел получил бы `load` при каждом повторном проходе
  (проход перезапускается на любой новый декод, не только «свой»).
* И `stream_image_sizes`, и `stream_image_errors` не дренируются за проход
  (тот же аргумент, что уже обосновывает недренирование `stream_image_sizes`,
  — BUG-735: узел, принявший тот же `src` ПОЗЖЕ, ещё должен получить свой
  `load`/`error`), и `relayout.rs`'s принудительное взведение `dirty`
  (BUG-730: новый узел на новом поддереве мог принять уже известный URL)
  расширено на `stream_image_errors` тем же условием.
* Оба новых поля заведены рядом с `stream_image_sizes`/`_dirty` во всех
  местах, где живёт per-tab состояние (`state.rs`, `page_state.rs`,
  `page_snapshot.rs`'s take/restore, три сброса в `resumed.rs`/`page_load.rs`
  навигационном пути/`tabs_cmd.rs`, инициализация в `window_mode.rs`) — то же
  зеркалирование, что уже требуют `stream_image_sizes`/`_dirty`.

**Живая проверка.** `verify_callback_import_preload_gaps.py --variant
img-onload-attr` (успех, dev-release): `ioa-script-listener-fired` и
`ioa-script-attr-fired` теперь печатаются (раньше — тишина, при том что
сервер видел `GET /vcip-pixel.png?script-made`). Новый вариант той же пробы,
`img-onerror-dynamic` (decode-неудача — скриптовый `<img>` с заведомо
404-путём, добавленный после `window.onload`): `ioe-listener-fired
complete=true naturalWidth=0` печатается (раньше — тишина, `img.complete`
оставался `false` навсегда).

**Не в этом срезе:** неприкреплённый `new Image()` (пункт 4 причины,
`verify_window_history_jsurl_gaps.py --variant canvas-misc`) не тронут —
`collect_image_requests` по-прежнему обходит только связное DOM-дерево.

Гейт: `cargo clippy -p lumen-shell --all-targets --features v8 -- -D
warnings` и `cargo clippy --workspace --all-targets -- -D warnings` чисто;
`scripts/scoped-test.sh` (база `90fcdc8cb`) — единственный красный
(`cpu_snapshots_match_references`, те же 7 файлов) — предсуществующий дрейф,
не регрессия (правка не трогает paint/layout).
