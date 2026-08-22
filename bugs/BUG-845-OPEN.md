# BUG-845 — EventSource: незавершённый блок пережившего разрыв соединения не выбрасывается, а склеивается со следующим соединением — и его `id:` уходит в `Last-Event-ID`

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, маркера намеренно нет: id уже атрибутированы BUG-844)
**Область:** `crates/network/src/sse.rs:393`–`400` (переподключение в `next_event` — парсер не сбрасывается), `crates/network/src/sse.rs:325` (`fill_queue`, ветка `n == 0`: EOF просто возвращает `Ok(false)`), `crates/network/src/sse.rs:122` (поле `id` пишется в `last_event_id` сразу при разборе строки)
**Владелец:** P1/P3 (`lumen-network`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Поток заканчивается на середине блока (ровно тело
`eventsource/format-data-before-final-empty-line.any.html`):

```
retry:400
data:test1

id:test
data:test2          ← пустой строки за этим блоком нет, соединение обрывается
```

Ожидание по спеке: диспатчится один `message` с `data === "test1"`; блок
`id:test`/`data:test2` отбрасывается вместе с соединением, `lastEventId`
остаётся пустым. Наблюдается: вторым сообщением приходит `data ===
"test2\ntest1"` и `lastEventId === "test"`, а на переподключении сервер видит
заголовок `Last-Event-ID: test`.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py --variant sse-incomplete-block`
(2026-08-22, dev-release, Linux, коммит `bafa603d9`, `--seconds 6`, страница
жива — 11 тиков), сырой лог `.tmp/psig-sse-incomplete-block.log`:

```
PROBE sse-message n=1 data=test1 id=
PROBE sse-message n=2 data=test2
test1 id=test              ← это одна строка: data === "test2\ntest1"
```

Пробный сервер записывает заголовки каждого соединения: первое — без
`Last-Event-ID`, все последующие — с `Last-Event-ID: test`, то есть id из
недиспатченного блока стал последним идентификатором события.

## Причина (локализована чтением кода)

`EventSource` в `sse.rs` держит один `SseParser` на весь жизненный цикл и при
переподключении (`sse.rs:393`–`400`) создаёт новое соединение, не трогая
состояние парсера: `data_buf`, `event_type` и `last_event_id` переезжают в
новое соединение как есть. HTML LS §9.2.5 («reestablish the connection»)
требует перед переподключением очистить буфер данных и буфер типа события
(сохраняется только *последний диспатченный* id).

Второй фасет — `sse.rs:122`: `id` пишется в `self.last_event_id` прямо при
разборе строки. По спеке значение поля сначала попадает в буфер id, а
«последним идентификатором события» становится только при диспатче блока
(§9.2.6, шаг «Set the last event ID string»).

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: все пять `eventsource`-id остатка
уже атрибутированы маркеру `eventsource-no-reconnect`
([BUG-844](BUG-844-OPEN.md)), и отдельного честного правила по исходнику для
этой формы не выводится. Баг заведён по прямому замеру — как BUG-825 и
BUG-829.

## Направление починки (не предписание)

При переподключении пересоздавать парсер (или очищать `data_buf`/`event_type`,
оставляя `last_event_id`), а поле `id` держать в отдельном буфере, копируя его
в `last_event_id` в `dispatch()`.

## Как проверить фикс

`tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
sse-incomplete-block` — ожидаются сообщения только с `data=test1` и пустым
`id`, а в списке соединений — ни одного `last-event-id=test`.
