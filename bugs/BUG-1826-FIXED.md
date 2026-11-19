# BUG-1826 — claude.ai: управляемый челлендж Cloudflare вешает движковый поток >4 мин (PoW-скрипт не завершается)

**Статус:** FIXED 2026-11-19 (P6, срез 5): спин устранён — toString-фасад кадровых мостов (`Symbol.toStringTag` + native-печать глобалов чужого realm); живой claude.ai проходит челлендж, движковый поток свободен. Остаток отпечатков — [BUG-1829](BUG-1829-OPEN.md).
**Заведён:** 2026-11-18 (Devin, при проверке BUG-1803 — системный прокси; маршрут уже исправлен, остался дефект движка)
**Компонент:** js/engine (выполнение обфусцированного скрипта `cdn-cgi/challenge-platform/.../chl_page`); семья BUG-935/BUG-1778/BUG-1800 (engine thread busy)

## Симптом

Живое окно `lumen.exe --maximized --mcp-live-port N https://claude.ai` (системный прокси Windows применён, BUG-1803):

- `claude.ai` → `302` → `/login` → `403` + страница «Just a moment...» (Cloudflare managed challenge, `[gate] cloudflare site=claude.ai подряд=1`).
- `GET /cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=…` → `200`, после чего `CSS пересобран после правки <style>: 210 правил` и `[chip] blank_screen site=claude.ai`.
- Дальше **одна JS-задача на движковом потоке идёт >135 с непрерывно** (зафиксировано `eval` через MCP: `engine thread busy: eval not run within 110.0 s (running task from crates\shell\src\app\about_to_wait.rs:339:13 for 135.1 s)`; первый непустой кадр — 238 с после старта процесса). Ошибок JS в `resource://console` нет — скрипт не падает, а крутится.
- Скрипт челленджа в Chrome проходит за секунды (proof-of-work + сбор фингерпринта); в Lumen он не завершается и за 4+ минуты — для пользователя это зависшая страница (сторож `script_stall` показывает «Страница не отвечает»).

## Гипотезы (не проверено)

- Busy-wait на shared-состоянии, которое выставила бы другая асинхронная задача, — а движковый поток один и занят этим скриптом (deadlock-by-cooperative-scheduling).
- Патологическая медлительность V8 на обфусцированном PoW (большие BigInt/typed-array циклы, отсутствие JIT-tier-up по какой-то причине).
- Ожидание события, которое в Lumen не наступает (`requestAnimationFrame`? `crypto.subtle`? `Atomics.wait` на main-потоке?), реализованное циклом.

## Репро

```
lumen.exe --maximized --mcp-live-port <N> https://claude.ai   # Windows, системный прокси включён
# через ~30 с: tools/call eval {code: "document.title", timeout_ms: 110000}
# → engine thread busy … running task from about_to_wait.rs:339 for >130 s
```

Скрипты пробы: `.tmp/mcp_probe2.py` в worktree `devin-proxy` (MCP JSON-RPC по TCP, token из stderr). Headless `--screenshot` челлендж не проходит by design (нет живого цикла кадров) — reproduce только в живом окне.

## Что делать

1. Снять, ЧТО крутит скрипт: stack sampling V8 (`SIGPROF`-аналога нет — кандидат: периодический `v8::StackTrace`/`CpuProfiler` из watchdog `script_stall`, или `LUMEN_SCRIPT_STALL_MS` + дамп текущего script source по `Script::Name`/`SourceURL`).
2. Если busy-wait — найти, какое API он ждёт, и почему тот не стреляет именно в этом рантайме.
3. Если медлительность — профиль (tracy/`--features tracy`), сравнить с Chrome на том же `chl_page` скрипте (он отдаётся по `ray`, можно скачать и прогнать офлайн в обоих движках).

## Срез 1 (2026-11-18/19, P6) — локализовано: вечный compute VM-интерпретатора, не busy-wait

Метод и инструмент: сэмплер `LUMEN_JS_STALL_SAMPLE_MS` (`v8_runtime/stall_sampler.rs`) расширен —
дамп исходника горячего скрипта (`LUMEN_STALL_DUMP_DIR`), статистика кучи на пробе, снимок
окружения изнутри зависшей задачи (`LUMEN_STALL_SNAPSHOT=1`, снимок исполняется колбэком
`request_interrupt` на JS-потоке посреди VM). A/B — стенд капч
(`scripts/captcha_stand.py`, корпус-строка `cf-managed-challenge`, `https://nopecha.com/demo/cloudflare`):
Lumen «отказ», Chrome для Testing 157 «пройдено» за ~15 c, тот же IP, свежие профили.

Установлено:

- **Не busy-wait и не блокировка**: зависшая задача — интерпретатор обфусцированной VM
  challenge (`switch`-диспетчеры `Fq`/`Bo`/`eK`/`Xmb`, таблицы операций `Fv`/`FK`, горячий кадр —
  16-битные хеш-раунды `65535&(a*(b>>>n^c)+(d>>>n)+((byte)<<8))`). Кадры меняются между
  пробами → VM прогрессирует по операциям, но не завершается. Задача одна непрерывная
  (сэмплы 2→258+ с без разрывов), DOM-активности и fetch'ей во время спина нет, воркеры
  не создаются (в логах `[worker-N]` пусто).
- **Пейлоад одинаковый**: orchestrate-ответ Chrome — 635 КБ, дампнутый из Lumen — 652 КБ
  (тот же блоб, пер-ray нонсы), челлендж-класс тот же: обеим сторонам сервер даёт 403 +
  `cType: 'interactive'`, 15 именованных полей `_cf_chl_opt`, страница ~6 КБ. Серверный
  скоринг не различает клиенты — расхождение в исполнении.
- **Батарея integrity-проверок VM проходит целиком** (проверено живым eval по списку
  декодированных строк — декодер `a0` реализован офлайн, `.tmp/stall/decode_a0.py`):
  `typeof PerformanceObserver === 'function'`, `new Worker(URL.createObjectURL(new Blob(
  ['"you"==="bot"'], {type:'text/javascript'})))` не бросает, `ReadableStream.prototype.pipeTo`
  — function, `performance.getEntries` — function, `BigInt`/`crypto.getRandomValues` на месте.
  Resource-Timing записи в Lumen есть (navigation/resource/longtask; нет только
  `visibility-state` — вероятно, не триггер).
- **Правка по снимку**: `performance.now()` отдавал сырой SystemTime (~0.25 мкс) против
  огрубленных до 100 мкс часов Chrome — квантовано `floor(x*10)/10` (HR Time L3 coarsening;
  монотонность сохранена, планировщик на точном нативе), тест
  `performance_now_is_coarsened_to_100_us`. На спин не повлияло (проверено живым прогоном
  после пересборки), но соответствие Chrome по таймерной точности сохранено как самостоятельное
  улучшение.
- Честный отпечаток (`cores=8/mem=16` через `data/fingerprint.toml`) спин не убирает.
- Офлайн-прогон дампа (localhost, оба движка) VM не заводит — origin-гейт (`bq()` проверяет
  `hostname` зоны), локальный бисект без прокси/hosts невозможен.

Осталось: вопрос «сходится ли VM когда-нибудь» (прогон 30–60 мин) и точка невозврата внутри VM —
дальше только полная деобфускация VM-программы либо побайтовый diff состояния с Chrome.
Скрипты среза: `.tmp/stall/` (`stall_run.py`, `bidi_probe.py`, `chrome_snapshot.py`,
`chrome_offline.py`, `chrome_net.py`, `chrome_html.py`, `local_repro.py`, `decode_a0.py`).

## Срез 2 (2026-11-19, P6) — «медленный compute» опровергнут; toString-проба нашла 71 не-нативную функцию

- **Скорость вычислений одинакова**: синтетический бенчмарк паттерна VM (16-битные хеш-раунды по
  `Uint8Array` внутри switch-диспетчера + BigInt-хвост, `bench_pow.html`) — стационар 1.3–1.5 мс/батч
  на обоих движках, BigInt 2.0 против 1.8 мс; Lumen даже греется быстрее (шагов tier-up хватает).
  Значит зависание — **поведенческая дивергенция** (VM уходит в вечную ветку), не скорость.
- **toString-проба** (`probe_native.py`, 80 платформенных API, Lumen против CfT 157 на одной машине):
  в Lumen **71 функция** отдают через `Function.prototype.toString` свой JS-исходник (fetch, XHR,
  createElement, setTimeout, ReadableStream.pipeTo, crypto.getRandomValues, PerformanceObserver,
  Worker — весь API), в Chrome все — `[native code]`. В таблице декодированных строк VM есть и
  `'toString'`, и `'[native code]'`, и имена проверяемых API (`setTimeout`, `open`, `send`, `now`,
  `createElement`, `encode`, `getRandomValues`, `PerformanceObserver`, `observe`, `Blob`, `Worker`,
  `pipeTo`, `ReadableStream`) — классическая проверка «нативности», помечающая Lumen как подменённый
  браузер. Батарея среза 1 проверяла только `typeof`, не печать функций.
- **Диф globalThis** (`probe_globals.py`): 378 имён только в Chrome (XR*/USB/Bluetooth/WebGL-классы,
  `clientInformation`, `external`, `moveBy`…), 58 только в Lumen (среди них чужие `_wc*`-хелперы
  WebCodecs, `SVG_NAMESPACE`/`MATHML_NAMESPACE`, глобальные `InvalidStateError`-классы,
  `SharedArrayBuffer`). **Пересечение таблицы строк VM с этим диффом — ноль**: VM не называет ни
  одного отсутствующего глобала, все 221 его идентификатор — методы/поля. Структурные дивергенты
  (SAB без crossOriginIsolated, методы WebGL/Crypto на инстансах без прототипов, нет
  RTCPeerConnection) выделены в BUG-1829.
- Прогон сходимости без фасада не доведён до 60 мин — закрыт результатом среза 3 (см. ниже).

## Срез 3 (2026-11-19, P6) — фасад `toString` (71 → 2), спин он не снимает

- **Реализация**: `shim/native_tostring_shim.js` — последний кусок и страничной склейки
  (`web_api_shim()`), и воркерной (`worker_exposed_shim()`): обход `globalThis` (+ прототип window,
  статики конструкторов, результаты геттеров — `document` и т.п.) в WeakMap фасадов формата Chrome
  (`function f() { [native code] }`, геттеры `function get x() { … }`) + патч
  `Function.prototype.toString` (чужие функции печатаются исходником как раньше — детект в обратную
  сторону исключён; не-функция бросает `TypeError`). Идемпотентен — повторный eval из
  `install_dom` (после `finalize_navigator_interface_v8`, до BiDi-preload и seal BUG-378) покрывает
  JS-шимы пост-программных модулей (Worker, SharedWorker, WebCodecs…). Живой бинарник: 71 → 2
  (остатки: `URL.createObjectURL` починен статиками в обходе уже после живого прогона,
  `navigator.mediaDevices.enumerateDevices` — ленивый модуль). Тесты `v8_native_tostring` (5),
  порядок склейки закреплён маркером; ловушка «шим после маски» — `subsystems/js.md`.
  267 тестов web_audio+worker+dom-подмножества зелёные; красные в полном параллельном прогоне —
  известные флаки BUG-1677/BUG-1789 (соло зелёные и с фасадом).
- **Живой прогон challenge с фасадом** (release, стенд nopecha, `converge_run.py`): **спин
  сохраняется** — сэмплы непрерывны 15→600 c, окно снято по таймауту. toString-чек — не единственный
  (или не тот) гейт: либо VM сверяет что-то ещё, либо тот же `[native code]`-чек но по объектам,
  которых фасад не касается (геттеры `Function.prototype.toString.call` покрыты; остаются V8-нативы
  Lumen с нестандартной печатью — не видно из пробы), либо условие выхода вообще не про
  нативность.
- Дальше: (а) честный оффлайн-харнесс — патч origin-гейта `bq()` в дампе + реплей записанной сети,
  инструментировка диспетчера VM, диф исполнения Lumen/Chrome до первой расходящейся ветки;
  (б) диф полей Resource-Timing orchestrate-записи Chrome/Lumen — у Lumen все milestone-таймстампы
  схлопнуты в `startTime` (`requestStart === responseStart === fetchStart`), `nextHopProtocol` пуст
  (в Chrome `h2`/`h3`) — VM мог бы читать нулевой TTFB как фейк; (в) проверка XHR-поверхности
  (`getResponseHeader`, `onreadystatechange`, `loadend`, `timeout`) и `crypto.randomUUID` — в
  списке имён VM.

## Срез 4 (2026-11-19, P6) — офлайн-харнесс: спин воспроизводится на localhost; deadlock отложенных загрузок; RTC-стаб продвигает VM

Метод: дамп `stall-script-145.js` (nopecha-челлендж среза 1) + страница дампа с `cZone`,
пропатченным под hostname харнесса (гейт `V4()`: `_cf_chl_opt.cZone === location.hostname` —
патч payload не понадобился). Сервер-харнесс `.tmp/stall/harness_serve.py`: `/cdn-cgi/*` отдаёт
payload с вклеенными трейсером и стабом, все прочие запросы логируются и отвечают 200 `{}`;
трассировщик `.tmp/stall/harness_trace.js` оборачивает setTimeout/XHR/fetch/crypto/performance/
DOM/RTC одинаково в обоих движках (toString-унификация), события — fetch-POST `/log` (sync XHR
в Lumen «отправляет», но ответы не доходят при занятом потоке — сам по себе побочный дефект).
Прогон: `.tmp/stall/run_offline.py` (CfT через CDP + Lumen через MCP + диф `analyze_trace.py`).

Установлено:

- **Спин воспроизводится офлайн** на localhost (без прокси и живого сайта): движковый поток
  Lumen занят одной задачей от `SETTIMEOUT(Ww,0)` (boot-колбэк) через `_lumen_tick_timers`,
  стек `_lumen_tick_timers → Ww → qrb → R → Xy → spb → CR → …` (VM-интерпретатор), горячие
  кадры меняются — т.е. это вычислительный busy-wait внутри задачи, а не ожидание событий.
  В Chrome тот же Ww завершается за ~200 мс.
- **Точка расхождения трейсов**: в Chrome после вставки скрытого измерительного iframe
  (`width=0 height=0 sandbox=allow-same-origin`, без src) VM **сразу читает
  `contentWindow`/`contentDocument`** (t≈66–128 мс, initial about:blank синхронен) и пишет в
  него; в Lumen обращений к content* нет вовсе — VM замирает между append и чтением.
  В Lumen геттеры content* ставятся по-элементно из Rust (`iframe_element.rs`) и возвращают
  фасад `frame_bridge` только после отложенного скана шелла (`web_api_shim_tail_b.js:2167`).
- **Deadlock отложенных загрузок (механизм подтверждён кодом)**: fetch динамического
  `<script src>` отложен в `setTimeout(0)` (`web_api_shim_mid.js:15878` — force-async, PERF-14),
  динамический iframe вообще не имеет ветки в шиме — его находит только скан шелла
  `poll_dynamic_frames` (`frame_dynamic.rs:51`), откладываемый при занятом документе
  (`try_lock`, BUG-935 срез 87, `frame_dynamic.rs:95-99`) и спавнящий сеть ещё на тик позже.
  Таймеры качает только свободный движковый поток (`about_to_wait.rs:315,339-349`). Итог:
  VM busy-wait'ит ресурс, чья загрузка не может начаться, пока поток занят самим ожиданием.
  Проба на свободном движке (`.tmp/stall/probe_dynscript.py`) показывает, что вне спина
  динамический скрипт Lumen грузит и исполняет нормально.
- **Отсутствие RTCPeerConnection — реальный гейт** (BUG-1829): одинаковый для обоих движков
  минимальный стаб (`.tmp/stall/harness_stub.js`, перекрывает и натив Chrome) продвигает VM
  Lumen существенно дальше — появляются `RTC.ctor` (`cfg` со `stun:stun.cloudflare.com`),
  `createDataChannel`, `createOffer`, `AEL icecandidate`, `SETTIMEOUT(5000)`, второй iframe —
  т.е. без стаба VM замирает раньше; с стабом затылок смещается к измерительному iframe (см.
  предыдущий пункт). Прелоад локальной копии api.js (исполнение до старта Ww) на спин не
  влияет — wait-условие VM не про готовность turnstile-глобала.
- **Resource-Timing записи** (опция (б) среза 3) в офлайн-прогоне доступны VM в обоих
  движках; у Lumen milestone-поля схлопнуты в `startTime`, `nextHopProtocol` пуст — остаётся
  паритет-дефектом, но спин не про него (VM не доходит до чтения записей orchestrate).
- Буфер трейсера из занятой задачи достаётся стлл-сэмплером: `SNAPSHOT_JS` дополнен дампом
  `__h_buf`/`__h_dbg`, состояния iframe (contentWindow/contentDocument/атрибуты),
  `window.turnstile` и onload-колбэка (эталон: `frames: []` — VM держит фреймы в отцеплённом
  контейнере, в Chrome так же; `turn: undefined` посреди спина).

Дальше: (а) движковая правка deferred-загрузок — стартовать сетевой fetch динамического
скрипта (шим) и динамического iframe (шелл) вне очереди движкового потока, синхронный
initial about:blank для вставленных iframe (contentDocument без ожидания скана);
(б) честная поверхность RTCPeerConnection (BUG-1829) как отдельная задача; (в) повторить
офлайн-прогон после (а): критерий — VM Lumen доходит до `XHR POST /cdn-cgi/…/fo` (отпечаток),
как в Chrome. Инструмент: `.tmp/stall/run_offline.py` (SKIP_CHROME=1 — только Lumen-плечо).

## Срез 5 (2026-11-19, P6) — FIXED: печать кадровых фасадов; живой claude.ai проходит челлендж

Гипотеза (а) среза 4 не подтвердилась: синхронный initial about:blank уже есть
(`initial_frames` BUG-1790), и трейс офлайн-прогона показывает, что VM читает
`contentWindow` синхронно в обоих движках. Реальный гейт — ПЕЧАТЬ фасада:

- **Инструмент**: Proxy-дневник `recordFacade` в трейсере харнесса (`.tmp`, не в
  коммите) — каждое чтение свойства на возвращённом content*-объекте логируется
  (`CWGET`), непронатные результаты `Function.prototype.toString` — (`TOSTR`).
- **Дивергенция 1**: `String(contentWindow)` — Chrome `[object Window]`, фасад
  Lumen `[object Object]` (то же для `contentDocument`). После чтения VM уходила
  в вечный перепроб (canvas+crypto+getEntries раз в секунду). **Фикс**:
  `Symbol.toStringTag` `'Window'`/`'HTMLDocument'`/`'Location'` на фасадах
  `frame_bridge.rs` (WebIDL @@toStringTag; прецеденты — credentials.rs, battery_bindings.rs).
- **Дивергенция 2** (следующий шаг VM): чтение `contentWindow.eval` и его
  `toString` — кловер моста печатал свой JS-исходник, Chrome —
  `function eval() { [native code] }`. **Фикс**: кловеры глобалов чужого realm
  получают имя оригинала и маркируются через хук `_lumen_tostring_mark`,
  экспортированный из `native_tostring_shim.js` (WeakMap замыкания шима
  недоступна снаружи). Ловушка: после глобального seal (BUG-378) динамический
  лукап ГОЛОГО имени поздние свойства не видит — хук читается как
  `globalThis._lumen_tostring_mark`; тест
  `initial_frame_facade_prints_like_chrome` держит оба фикса
  (`crates/js/src/dom/tests/v8_native_tostring.rs`).
- **Результат офлайн** (`run_offline.py`, полный прогон): спин исчез — VM
  проходит оба прежних гейта, доходит до `XHR POST /eb` (фингерпринт, ~600 мс
  после orchestrate) и встаёт в тот же 550-мс retry-цикл, что Chrome
  (интерактивное ожидание); движковый поток свободен с 10-й секунды.
  Остаток офлайна: реальный turnstile `api.js` (харнесс ходит за ним в интернет)
  не находит свой onload-колбэк (`Unable to find onload callback
  _ce1dc0c1663c982f624a9bda`), `POST /fo` не уходит — в синтетике отпечаток
  всё равно не проходит (RTC-стаб, фейковый сервер, cores/mem реальные).
- **Живой прогон claude.ai** (release, `--mcp-live-port`, `.tmp/stall/live_check.py`):
  спина нет — `eval` отвечает каждые 15 c на протяжении 130 c, титул
  «Sign in or create an account | Claude.ai» — челлендж пройден, страница
  загрузилась. Дефект (одна JS-задача >135 c, «Страница не отвечает») устранён.
- Гейт: `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
  чисто; `v8_native_tostring` 6/6, `frame` 155, `tostring` 8, `content_document` 2 —
  зелёные (один флак параллельного прогона, соло зелёный — известные BUG-1677/BUG-1789).
