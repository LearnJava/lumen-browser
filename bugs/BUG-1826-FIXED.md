# BUG-1826 — claude.ai: управляемый челлендж Cloudflare вешает движковый поток >4 мин (PoW-скрипт не завершается)

**Статус:** FIXED 2026-11-19 — закрыт двумя независимыми слоями: (1) P6, срезы 1–5 — JS-фингерпринт: toString-фасад кадровых мостов (`Symbol.toStringTag` + native-печать глобалов чужого realm), живой claude.ai проходит челлендж, движковый поток свободен; (2) Devin (worktree devin-proxy) — сетевой фингерпринт Chrome 157: akamai H2 SETTINGS, HTTP-заголовки/Client Hints, `Accept-Encoding+zstd`, TLS ClientHello rustls→BoringSSL. Остаток отпечатков — [BUG-1829](BUG-1829-OPEN.md).
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

## Разбор 2026-11-18 (продолжение, worktree devin-proxy)

### Измеренные факты

- **Это не lock/event/worker-ожидание.** Stall-сэмплер (`LUMEN_JS_STALL_SAMPLE_MS`) показывает непрерывное исполнение одной JS-задачи на `lumen-v8`: >630 с в прогоне 18.11, CPU процесса ~1.3 ядра. Стек — обфусцированный VM-интерпретатор Cloudflare.
- **`--prof` профиль** (`node --prof-process isolate-*.log`, 20898 тиков): ~38% nonlib-времени в одной функции `nY` (диспетчер байткод-VM, `chl_page.js:2:294556`, цикл `case` по `lw[...]`), GC — только 3.5% тиков. Т.е. узкое место — собственно исполнение JS-байткода VM, а не GC и не ожидание.
- **Микробенчмарки Lumen vs Node 26** (одинаковый V8-класс кода, `--mcp` eval):
  - элементный доступ typed array — паритет или быстрее (u8 reuse: 8.8 vs 11.5 мс на 3M);
  - **аллокация малых typed array (≤64 Б) — ~6.6x медленнее**: `new Uint8Array(8)` escaped = 1.84 мкс в Lumen против 0.28 мкс в Node; порог на-куче в Node подтверждён (≤64 Б быстро, ≥128 Б — общий off-heap путь ~2.3 мкс);
  - `ArrayBuffer(64)` сам по себе одинаково медленный в обоих (~2 мкс) — это malloc backing store;
  - `performance.now`/`Date.now`/`Math.random` — в пределах 1.3–1.8x; `crypto.getRandomValues` ~2x; atob после нативной реализации ~13x медленнее Node на ~1 КБ, но это не доминанта.
- **Вывод по механизму (измерено + инференс):** в сборке Lumen `rusty_v8` без pointer compression (`v8_enable_pointer_compression`/`sandbox` недоступны: prebuilt `_ptrcomp`/`_sandbox` для windows-msvc отдают 404, сборка из исходников падает на `gn gen` — sandbox требует libc++ hardening, нет `dbghelp.dll`) отсутствует on-heap fast path для малых typed array — V8 держит их данные в куче только при pointer compression (кодирование `base_pointer`/`external_pointer` рассчитано на сжатые указатели; compile-time `V8_TYPED_ARRAY_MAX_SIZE_IN_HEAP`, runtime-флага нет). Cloudflare VM делает массу мелких TA-аллокаций → каждое ~2 мкс вместо ~0.3 мкс.
- Chrome/Edge/Brave проходят тот же managed challenge за ~5–20 с (замеры через CDP/Playwright в этой сессии); Lumen — >10 мин на одной задаче. Класс «custom engine» у CF официально «limited support» (developers.cloudflare.com/cloudflare-challenges/reference/supported-browsers/), но Edge тоже висел ~120 с — поведение зависит и от ветки челленджа.

### Что уже исправлено в этой ветке (не причина зависания, но реальные дефекты)

- `native_mask_shim.js`: найденные при обходе функции не ставились в очередь обхода → методы прототипов (`TextEncoder.prototype.encode` и др.) оставались немаскированными; исправлено, все 10 atob/btoa + mask-тестов зелёные.
- Page-side `atob`/`btoa` переведены на Rust-ноды worker'ов (`WORKER_ATOB_BTOA_SHIM` через `wrap_for_container` — без него `_lumen_atob_impl` не виден: page-скоуп резолвит `_lumen_*` через internal container, не через global). Микрозамер: native atob ~в 8x быстрее старого JS-шима.
- `LUMEN_V8_FLAGS` — произвольные флаги V8 до `V8::initialize` (диагностика).

### Открытый вопрос

Главный кандидат на корень — **отсутствие pointer compression** (нет on-heap TA + больше heap/GC). Для его включения нужен либо prebuilt `_ptrcomp` (отсутствует для windows-msvc), либо сборка V8 из исходников с `v8_enable_pointer_compression=true, v8_enable_sandbox=false` — прошлый заход падал на sandbox; ptrcomp без sandbox ещё не пробовали (dbghelp.dll может понадобиться снова). Альтернатива — принять как известный пробел движка и зафиксировать.

### Worker-наблюдение (2026-11-18, isolate-tagged stall sampler)

- Stall-сэмплер теперь печатает указатель изолята (`isolate {:?}` в `stall_sampler.rs`); все `V8JsRuntime` (страница, worker, SW) зовут его线程 `lumen-v8`, поэтому раньше изоляты было не различить.
- Прогон 18.11 ~3: **все** сэмплы (>140 с задачи) — на одном изоляте `0x15d3e271000`; worker `id=0` (`blob:`) стартовал, выполнил top-level и **ни разу не получил сообщение** (`[worker-debug] got Post` — 0 записей). Второй изолят в сталлах не появляется → JS worker'а простаивает.
- postMessage page→worker сериализуется через `JSON.stringify` (+ sentinel только для OffscreenCanvas, `worker_shims.rs:570-609`): `SharedArrayBuffer`/`ArrayBuffer`-transfer через структурный клон **не поддерживаются** — SAB доехал бы как `{}`. Если CF-протокол ждёт SAB+Atomics handshake, main-thread синхронно ждать worker может только через shared memory — этот путь в Lumen отсутствует → правдоподобный механизм зависания: main крутит VM-цикл, worker пуст.
- Профиль (`--prof`): 56% JavaScript / 44% native (V8 C++ + Rust-коллеры) / GC 3.5%. Т.е. стоимость — не только «медленный JS», но и дорогие переходы JS↔native (шимоиды API, аллокацииBackingStore).

### Обновление 2026-11-19 (worktree devin-proxy)

- **GC/semispace-тюнинг не помог:** повторный живой прогон с `LUMEN_V8_FLAGS` (semi-space) — задача всё ещё идёт >700 с без завершения. Вывод: узкое место не в размере new-space.
- **Worker окончательно декодирован:** за весь прогон зарегистрирован ровно один blob — 13 байт `"you"==="bot"` (канарейка). Page→worker сообщений не было. Вся работа челленджа — на main-изоляте; worker-изолят спит в task loop. Гипотеза «второй фронт в worker» закрыта.
- **C) Разбивка native-тик:** в `--prof`-профиле доминирует `JS: ~get :743:20` (51% нативных тиков) — но это функция `get` из eval'нутого скрипта без имени (наши шимы), НЕ вызов Web API из CF-кода. Конкретного горячего `_lumen_*`-биндинга нет: 44% native — это V8 runtime (аллокации, IC) под JS-фреймами VM. Инструментация call-stats добавлена в `v8_compat.rs` (env-gated), сборка бинаря идёт.
- **A) ptrcomp из исходников — вторая попытка:** `gn gen` прошёл после junction для dbghelp; `ninja` упал на отсутствующем `third_party/icu/common/icudtl.dat` (crate `v8-150.1.0` не шипит ICU data file). Перезапуск с `GN_ARGS="icu_use_data_file=false"` — ICU data линкуется из исходников, файл не нужен. Сборка V8 идёт (часы).
- **Рефрейминг длительности:** managed challenge конечен и у настоящих браузеров — Brave прошёл за ~114 с, Edge не уложился в 120 с. Lumen при ~4–6x медленном JS даёт оценку 8–12 мин; прогон убитый на 640 с мог быть близок к финалу. Текущий прогон идёт >700 с — проверяем, завершится ли вообще.

### Call-stats результат (2026-11-19, LUMEN_NATIVE_CALL_STATS=1, живой челлендж)

- Инструментированный dev-release бинарь, прогон с challенджем: топ нативов на 20/40/60 с **идентичен и не растёт** — `_lumen_atob_impl ×906`, `_lumen_get_parent ×367`, `_lumen_get_attr ×239`, `_lumen_query_selector ×171`, `_lumen_now_ms ×148`… **Все они — однократные вызовы фазы bootstrap; во время сталла VM не делает НИ ОДНОГО нативного вызова.**
- Вывод: горячий цикл — чистый JS внутри байткод-VM Cloudflare (SHA/бигит-арифметика в интерпретаторе), без обращений к Web API. Оптимизация шимов/нативов **не повлияет** на длительность. Остаётся только скорость самого V8 (pointer compression / tiering) либо незавершимость по дизайну.
- Два независимых прогона задачи: >55 мин и >65 мин непрерывного CPU ~1 ядро, память стабильна (~685/857 МБ) — без аллокаций, чистый compute-loop.
- Проверка гипотезы «медленный но конечный»: Brave проходит managed challenge ~114 с; при замеренном разрыве JS ~4–6x ожидалось бы ~10 мин. Фактические >60 мин выводят за рамки «просто медленно» — либо сложность для этого egress-IP аномально высока, либо цикл не завершается (бесконечный spin / неверно декодированные параметры PoW).
- ptrcomp-сборка V8 из исходников: решены icudtl.dat (взят icudt77l.dat из ICU 77.1 data-bin-l), 252 отсутствующих chromium_crates_io крейта (выкачаны .crate с crates.io по cargo_pkg_version из BUILD.gn), dbghelp (fakesdk + WINDOWSSDKDIR), MAX_PATH (CARGO_TARGET_DIR=C:\lgt — короткий путь, без junction). ninja пошёл.

### Финализация 2026-11-19 (все пункты A–D пройдены)

- **ptrcomp-сборка V8 — тупик.** После ~15 попыток: icudtl.dat решён, 252 chromium-крейта выкачаны, dbghelp обойдён fakesdk+WINDOWSSDKDIR, MAX_PATH обойдён `CARGO_TARGET_DIR=C:\lgt`. ninja упал на generated static assertions `JSInterceptorMap` (offsets расходятся на 3 байта — несовместимость сгенерированного кода/тулчейна windows-msvc), последняя попытка — на создании symlink `T:\debug\gn_root` (нет прав, `symlink_dir failed: 1314`). Prebuilt `_ptrcomp`/`_sandbox` для windows-msvc — 404. **Pointer-compressed V8 для этого таргета недостижим** текущим тулчейном.
- **Решающий контр-бенчмарк:** alloc-heavy цикл с escape (2M итераций `[o.a,o.b,o.c]` + объект + периодический push в global): **Lumen 51 мс vs Node 192 мс — Lumen в ~4 раза быстрее**. Элементный доступ и примитивы — паритет. Значит сырой скорости V8 как единственного объяснения >60-минутного сталла недостаточно.
- **Итоговая картина:** сталл — чистый JS-цикл байткод-VM Cloudflare без единого нативного вызова (call-stats), при паритетной скорости движка. Наиболее вероятная причина — **серверная сторона**: CF назначает Lumen челлендж, не завершающийся за разумное время (аномально высокая сложность PoW или soft-ban через незавершимый челлендж для клиента с недоверенным фингерпринтом: TLS-стек `rustls`, не-Chromium сигналы). Косвенное подтверждение: на «чистых» exit-нодах прокси Lumen получает `/login` 200 без челленджа вообще (скриншот пользователя 18.11 — логин-форма отрисовалась).
- **Направление исправления** — сетевой слой/фингерпринт (JA3/TLS, HTTP/2 fingerprint), не JS-движок. В рамках js-крейта дефектов, блокирующих челлендж, не найдено.
- `LUMEN_V8_FLAGS="--always-turbofan --no-maglev"` не применился: `--always-turbofan` удалён из V8 150 (`unrecognized flag`).

### РЕШЕНО 2026-11-19 — HTTP/2 SETTINGS фингерпринт (корень найден и устранён)

**Эксперимент, давший ответ:** прямые TLS/HTTP-клиенты через тот же системный прокси (`127.0.0.1:12334`) на `claude.ai` — Go `crypto/tls`, utls `HelloChrome_Auto`, utls `HelloRandomized` — все получали чистый `302 → /login` без челленджа, пока Lumen на том же exit-IP получал `403` + `chl_page`. Значит триггер — не TLS ClientHello и не exit-IP, а то, что отличает Lumen на уровне HTTP/2.

**Замер фингерпринтов** (`tls.peet.ws/api/all`, живой Lumen vs Chrome 157):

- TLS JA3/JA4 различаются (rustls vs BoringSSL: нет GREASE, MLKEM, boringssl-расширений) — **но не являются триггером**: Go-клиент с generic TLS прошёл.
- **Akamai HTTP/2 fingerprint — был неверным.** Lumen слал `1:65536;2:1;3:1000;4:6291456;5:16384` (ENABLE_PUSH=1, MAX_CONCURRENT_STREAMS=1000, MAX_FRAME_SIZE=16384, без MAX_HEADER_LIST_SIZE) — «Chrome-профиль» в `h2_settings.rs` содержал устаревшие значения. Реальный Chrome 157 шлёт `1:65536;2:0;4:6291456;6:262144` (ENABLE_PUSH=0, без MAX_CONCURRENT_STREAMS/MAX_FRAME_SIZE, с MAX_HEADER_LIST_SIZE=262144).
- Заголовки тоже отличаются (cache-control, sec-ch-ua без Google Chrome, accept-encoding `br, gzip, deflate` без zstd, нет upgrade-insecure-requests/sec-fetch-user) — но их подмена в Go-клиенте челлендж не вызывала и не снимала: триггер был именно в SETTINGS.

**Фикс:** `crates/network/src/http/h2_settings.rs` — Chrome/Strict/Edge профили приведены к измеренным значениям Chrome 157 (`enable_push=false`, `max_concurrent_streams=None`, `max_frame_size=None`, `max_header_list_size=Some(262144)`); `crates/network/src/h2/conn.rs` — SETTINGS frame теперь эмитит опциональные поля по профилю.

**Проверка:** akamai fingerprint Lumen после фикса байт-в-байт совпал с Chrome 157; `claude.ai` — 4/4 живых прогона `--maximized` + `LUMEN_NO_ADBLOCK=1`: `GET /login → 200`, страница логина (шрифты, hero, edge-api bootstrap) грузится без единого челленджа. До фикса на том же прокси — стабильный 403/chl_page.

**Остаточные отличия фингерпринта** (не блокеры на текущем exit-IP, кандидаты на follow-up при рецидивах): TLS ClientHello rustls vs BoringSSL, порядок/состав HTTP-заголовков, sec-ch-ua brands (нет "Google Chrome"), accept-encoding без zstd.

### Follow-up 2026-11-19 — HTTP-заголовки доведены до набора Chrome 157 (задача B)

Цель — снять остаточные отличия заголовочного фингерпринта, чтобы не ловить челлендж на «плохих» exit-IP. Всё выверено по замеру `tls.peet.ws/api/all` на реальном Chrome 157.

- **Порядок и состав H1/H2-заголовков навигации** ([crates/network/src/http/headers.rs](../crates/network/src/http/headers.rs)): `Host, Connection, Sec-CH-UA/-Mobile/-Platform, Upgrade-Insecure-Requests: 1, User-Agent, Accept, Sec-Fetch-Site/-Mode/-User: ?1/-Dest, Accept-Encoding, Accept-Language` (+ DNT/Sec-GPC только у Strict). Убраны `Cache-Control: max-age=0` (Chrome шлёт его только на reload — reload-путь и так добавляет `Cache-Control: no-cache` через `cache_bypass_headers`) и перенос Sec-Fetch-* с конца блока на позицию после Accept. Edge — тот же набор.
- **`Sec-Fetch-User: ?1`** добавлен в navigation-блок; на сабресурсах/fetch/XHR он снимается через `X-Lumen-Drop` в [crates/network/src/fetch_metadata.rs](../crates/network/src/fetch_metadata.rs) — Chrome его на не-навигациях не шлёт. `Upgrade-Insecure-Requests` наоборот оставлен на всех запросах: Chrome шлёт его и на сабресурсах.
- **Client Hints brands** ([crates/core/src/identity.rs](../crates/core/src/identity.rs)): Chrome-профиль теперь отдаёт реальный порядок Chrome 157 — `GREASE, "Google Chrome", "Chromium"` (раньше "Google Chrome" отсутствовал, а GREASE вставлялся формулой не на ту позицию). `ChromeBrandGoogle` совпадает с `ChromeUnbranded` по построению. `navigator.userAgentData.brands` и `Sec-CH-UA` берутся из одного `Identity`, противоречий нет.
- **`Accept-Encoding` + zstd**: добавлен `ZstdContentDecoder` ([crates/network/src/zstd.rs](../crates/network/src/zstd.rs)) на pure-Rust `ruzstd 0.9.1` (только декомпрессия; тест на эталонном фрейме, сжатом системным zstd). Порядок регистрации декодеров в driver — `gzip, deflate, br, zstd`, как у Chrome (было `br, gzip, deflate`). Рекламировать zstd без декодера было нельзя — сервер мог бы отдать неснимаемое сжатие.

Тесты: `http_client_fingerprint_profile` (новый `chrome_headers_match_measured_chrome_157_order` + обновлённые), unit-тесты `headers.rs`/`fetch_metadata.rs`/`identity.rs`, JS-тесты `ua_client_hints`/`v8_bug624` (brands.length 2→3). `cargo clippy -p lumen-network -p lumen-core -p lumen-driver --all-targets -D warnings` чист.

### Follow-up 2026-11-19 — TLS-транспорт переведён на BoringSSL (задача A, решающая)

Последнее крупное расхождение фингерпринта — TLS ClientHello: rustls не умеет GREASE, перестановку расширений, X25519MLKEM768 и полный набор Chrome-расширений (JA3 `8b54b221…`/JA4 `t13d1011h2` против Chrome `760fc7ae…`/`t13d1517h2`).

**Решающий A/B-эксперимент** (один exit-IP `85.208.119.27`, одна минута): spike на crate `boring` (BoringSSL) → `claude.ai/login` **200 без челленджа**; Lumen на rustls в то же время → `403 → chl_page`; настоящий Chrome → 200. Вывод: CF edge ветвит именно по TLS-фингерпринту — с Chrome-grade hello челлендж не выдаётся вообще.

**Реализация** — [crates/network/src/tls/boring.rs](../crates/network/src/tls/boring.rs) + точка подключения в [crates/network/src/lib.rs](../crates/network/src/lib.rs) (`connect_inner` и HTTP-CONNECT-туннель):

- `RawStream::Tls` теперь `boring::ssl::SslStream<TcpStream>` — Read/Write-совместим, h1/h2-mux/websocket работают без изменений.
- ClientHello: cipher list Chrome, `signature_algorithms` без PQ/`rsa_pkcs1_sha1`, groups `X25519MLKEM768:X25519:P-256:P-384`, ALPN `h2,http/1.1` (Tor/h1-фолбэк — `http/1.1`), TLS 1.2+1.3 для Standard / 1.3-only для Strict/Tor. BoringSSL сам добавляет GREASE и перестановку расширений.
- **Верификация сертификатов не портирована, а переиспользована**: handshake идёт с `SslVerifyMode::NONE`, затем `LumenVerifier::verify_server_cert` прогоняется на DER-цепочке post-handshake — webpki path validation, AIA, NUC, stapled-OCSP (`Revoked` hard-fail + `record_stapled`), CT и bypass работают без изменений. Запрос OCSP-staple — `set_status_type(StatusType::OCSP)` до handshake.
- h3/QUIC и DNS-over-TLS остаются на rustls (QUIC-бэкенда у BoringSSL здесь нет; DoT не виден edge сайта).
- Новая зависимость `boring = "5.5.1"` (boring-sys собирает BoringSSL: нужны cmake + nasm + MSVC).

**Проверка** (тот же «плохой» exit-IP, `--maximized` + `LUMEN_NO_ADBLOCK=1`): `claude.ai` — 3/3 прогона `302 → /login → 200` + вся страница логина (шрифты, hero, edge-api bootstrap) **без единого челленджа**; `whatismyipaddress.com` — `200` сразу, весь сайт с ресурсами, челленджа не было вообще (до фикса — `Cf-Mitigated: challenge`/403). Сертификатная политика проверена живьём: `expired.badssl.com` → interstitial «certificate expired» (`Error::CertInvalid`), ретрай после транзиентного EOF отработал.
