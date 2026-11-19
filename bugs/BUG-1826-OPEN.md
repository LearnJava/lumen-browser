# BUG-1826 — claude.ai: управляемый челлендж Cloudflare вешает движковый поток >4 мин (PoW-скрипт не завершается)

**Статус:** OPEN
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
