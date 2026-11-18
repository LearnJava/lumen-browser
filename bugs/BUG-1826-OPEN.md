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
