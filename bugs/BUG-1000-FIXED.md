# BUG-1000 — `SharedWorker` не закрывается на уходе документа: соединение живёт до смерти процесса

**Статус:** FIXED 2026-09-29 (P6)
**Заведён:** 2026-09-05 (P3, побочная находка при диагностике [BUG-988](BUG-988-FIXED.md))
**Область:** `crates/js/src/shared_worker.rs` (`hub_v8`/`HUB_V8`, `connect_shared_worker_v8`, `close_shared_worker_port_v8`) / `crates/js/src/v8_runtime/runtime.rs` (`V8JsRuntime::drop`)
**Владелец:** P3, исправлено P6

## Симптом

`new SharedWorker(...)`, подключённый со страницы, продолжает работать на своём
потоке сколь угодно долго после того, как страница, которая его создала, ушла
(навигация, закрытие вкладки) — вплоть до смерти всего процесса. Если скрипт
воркера сам держит таймер/интервал, это постоянно активный поток, ничем не
связанный с жизнью какой-либо конкретной страницы.

## Причина

`HUB_V8` (`crates/js/src/shared_worker.rs::hub_v8`) — процесс-глобальный
`static OnceLock<Mutex<HashMap<String, SharedWorkerThread>>>`, ключ — origin+имя
воркера. Это осознанное проектное решение (`SharedWorker` по спеке переживает
одну конкретную страницу), но единственный путь, который снимает запись —
`close_shared_worker_port_v8` (`_lumen_sw_close`) — вызывается **только**
явным `port.close()` из самого JS. Ни на навигации, ни на закрытии вкладки, ни
на уничтожении документа ничего не итерирует открытые порты этой страницы и не
шлёт `_lumen_sw_close` за неё — комментарий в коде прямо фиксирует половину
контракта («the worker thread itself stays alive for other clients») и не
покрывает «клиентов больше нет».

Отличается от [BUG-988](BUG-988-FIXED.md) (обычный `Worker`, утекавший
только через `park_current_page`/bfcache и уже починенный) тем, что здесь утечка
**не зависит от park вообще** — она происходит при любом уходе со страницы,
паркуется та или нет.

## Почему это не просто «баг с малой ценой»

Если сайт использует `SharedWorker` для меж-вкладочной синхронизации/heartbeat
(частый паттерн у аналитики/рекламы), поток тикает с частотой, заданной его
собственным `setInterval` (тот же пол в 4 мс из `WORKER_TIMERS_SHIM` после 5
уровней вложенности, что и у обычного `Worker`), неограниченно долго — до конца
жизни процесса, а не до конца жизни конкретной вкладки.

## Первый шаг

Нужен явный хук на «документ выгружается» (там же, где сегодня стреляет
`_lumen_unload_document`/`pagehide`, `crates/js/src/shim/web_api_shim_mid_b.js`),
закрывающий все `SharedWorker`-порты, открытые ЭТОЙ страницей — потребуется
JS-сторонний реестр открытых портов (аналог `_workerRegistry` у обычного
`Worker`, `crates/js/src/worker.rs`), которого сейчас нет: `SharedWorker`
клиентская сторона хранит порты только неявно, через замыкания в
`connect`/`postMessage`. Проверить также, не стоит ли распространить на
`SharedWorker` тот же park-blocker, что BUG-988 завёл для обычного `Worker`
(`_lumen_bfcache_blocked()`) — это не решает саму утечку (она не зависит от
park), но не даст `SharedWorker`-странице попасть в whole-runtime park
одновременно с висящим портом.

## Сырые данные

Найдено статическим разбором `crates/js/src/shared_worker.rs` при диагностике
BUG-988 (P3 2026-09-05), живым прогоном не подтверждено.

## Исправление 2026-09-29 (P6)

Новое поле `V8JsRuntime::shared_worker_client_ports` (`SharedWorkerClientPorts`,
`Arc<Mutex<HashMap<u32, String>>>` — port id → identity key) — реестр каждого
порта, который эта конкретная страница открыла через `_lumen_sw_connect`.
Дедицированный `Worker` разрывает связь бесплатно: его `WorkerRegistry` —
обычное поле `V8JsRuntime`, роняется вместе с ним. У `SharedWorker` хаб
процесс-глобальный, поэтому `Drop for V8JsRuntime` (`v8_runtime/runtime.rs`)
теперь явно зовёт `close_all_client_ports_v8`, которая для каждого
запомненного порта шлёт тот же `_lumen_sw_close`, что и явный `port.close()`
из скрипта.

На стороне воркера `run_shared_worker_thread_v8`'s обработчик `SwInMsg::Close`
проверяет, не опустела ли карта `ports` ЭТОГО воркера после удаления записи —
если да, взводит `close_flag` и поток завершается сам, вместо бесконечного
ожидания в `HUB_V8`. Новый клиент того же `key` после этого просто порождает
свежий поток (`connect_shared_worker_v8` уже обрабатывал мёртвый `tx` как
кейс пересоздания).

Пункт про park-blocker из «Первого шага» закрыт архитектурно, без отдельного
кода: `park_current_page` (`crates/shell/src/lumen/bfcache.rs`) клонирует
`Arc`-хендл рантайма в `ParkedPage`, а не дропает его — запаркованная
страница просто не проходит через `Drop`, значит её `SharedWorker`-порты не
закрываются, пока она не будет вытеснена или заменена по-настоящему. Отдельно
распространять `_lumen_bfcache_blocked()` на `SharedWorker` не нужно: сам факт
парковки уже не рвёт соединение.

Регрессионный тест: `v8_dropping_the_page_runtime_closes_its_shared_worker_ports`
(`crates/js/src/shared_worker.rs`) — открывает `SharedWorker`, роняет рантайм,
затем ретраями (до 50×10 мс, поток закрывается асинхронно) убеждается, что
свежий `new SharedWorker(тот же key)` порождает НОВЫЙ поток (счётчик `n`
внутри воркера стартует заново с 1, а не продолжает с прежнего значения).

Гейты: `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` чисто; `cargo test -p lumen-js --features v8-backend --lib
shared_worker` — 34/34 зелёных; `scripts/scoped-test.sh main` — все затронутые
крейты зелёные; `LUMEN_PROFILE=dev-release python graphic_tests/dump_golden.py`
— 12/12.
