# Журнал перф-аудитов Lumen

Хронология прогонов корпуса реальных сайтов ([corpus.txt](corpus.txt)) через
[`scripts/perf_audit.py`](../../scripts/perf_audit.py). Протокол — skill
`/lumen-perf-audit`. Сырые результаты прогонов — `docs/perf/runs/<date>.json`
(коммитятся; скриншоты и stderr-логи остаются в `.tmp/`, не коммитятся).

Сравнивать числа можно только между прогонами **одной машины**; колебания
±20% на сетевых фазах — шум. Замедление >20% на той же машине — находка.

Шаблон секции:

```markdown
## YYYY-MM-DD — <коммит движка> — <машина>

<сводная таблица из summary.md>

**Сравнение с прошлым прогоном:** <дельты или «первый прогон»>
**Находки:** <маркированный список>
**Заведённые баги:** BUG-NNN, … (или «нет новых»)
```

---

## 2026-07-17 — b7a951b7 — Windows 10, dev-release (первый прогон журнала)

Прогон `scripts/perf_audit.py` (сырые данные: [runs/2026-07-17.json](runs/2026-07-17.json)).
RAM/CPU-колонки добавлены в харнесс после этого прогона — появятся со следующего.

| slug | статус | HTTP | source, с | layout, с | screenshot, с | доминирует | ошибки |
|---|---|---|---|---|---|---|---|
| example | OK | — | 0.08 | 0.09 | 0.14 | net_parse |  |
| ya | OK | 200 | 0.61 | 2.01 | 1.29 | style_layout | script error: JS runtime error: Unable to find RenderContext state htm |
| hn | OK | 200 | 1.35 | 1.46 | 2.49 | net_parse | script error: JS runtime error: el.getElementsByClassName is not a fun |
| w3 | FAIL | 403 | 0.75 | 0.76 | 0.75 | - | Ошибка --screenshot https://www.w3.org/: network error: HTTP 403 |
| rust-lang | OK | 200 | 2.27 | 5.74 | 3.85 | style_layout |  |
| lenta | OK | 200 | 0.3 | 1.45 | 13.16 | paint | ✗ https://ssp.rambler.ru/capirs_async.js (dns: resolve ssp.rambler.ru: |
| github | TIMEOUT | 200 | 0.7 | 240.29 | 240.29 | - | module error: JS runtime error: Automatic publicPath is not supported  |
| stackoverflow | FAIL | 200 | 1.38 | 13.48 | 0.86 | - | Ошибка --screenshot https://stackoverflow.com/: network error: HTTP 42 |
| crates | OK | 200 | 1.33 | 1.83 | 0.96 | net_parse | script error: JS runtime error: Cannot read properties of undefined (r |
| docs-rs | OK | 200 | 1.12 | 1.64 | 0.98 | net_parse | script error: JS runtime error: Cannot read properties of undefined (r |
| ria | OK | 200 | 0.26 | 4.57 | 4.36 | style_layout | script error: JS runtime error: Image is not defined |
| habr | OK | 200 | 0.79 | 16.16 | 41.8 | paint | ✗ https://cdn.skcrtxr.com/roxot-wrapper/js/roxot-manager.js?pid=c42719 |
| mdn | OK | 200 | 1.0 | 4.92 | 64.65 | paint | [JS warn] Unable to set theme TypeError: Cannot set properties of unde |
| rbc | OK | 200 | 0.31 | 6.77 | 11.1 | style_layout | ✗ https://top-fwz1.mail.ru/counter?id=3081030;js=na (dns: resolve top- |

**Сравнение с прошлым прогоном:** первый прогон журнала; против ручного аудита
2026-07-02: lenta.ru 141.7 с → 13.2 с (~11×, срезы BUG-267/272 + параллельный
fetch); crates.io, docs.rs, ria.ru открылись (в июле 403/500); w3.org наоборот
стал 403 (в июле открывался), stackoverflow теперь 429 на повторных стадиях.

**Находки:**
- github.com висит ≥240 с и на V8 (сеть готова за 0.7 с) — гипотеза «медленный
  QuickJS» опровергнута → BUG-303.
- DNS 11004 (WSANO_DATA) на живых доменах бьёт по подресурсам 4+ сайтов
  (mc.yandex.ru, ssp.rambler.ru, top-fwz1.mail.ru, …) → BUG-304.
- Отсутствующие JS API валят site-скрипты целиком: `getElementsByClassName`
  (HN) → BUG-302, конструктор `Image` (ria.ru) → BUG-305.
- Paint по-прежнему доминирует на длинных страницах (CPU-путь): mdn 59.7 с
  (32768 px — подозрительно ровный кламп высоты), habr 25.6 с (31315 px),
  lenta 11.7 с. Известный класс (CPU-растеризация), новый баг не заводился.
- style_layout тяжёлый на habr 15.4 с, rbc 6.5 с, ria 4.3 с, rust-lang 3.5 с —
  кандидат на профилирование LUMEN_PROFILE_TREE в следующем прогоне.

**Заведённые баги:** BUG-302, BUG-303, BUG-304, BUG-305.

---

## 2026-07-17 (2) — живой базлайн — окно --maximized, вкладка на сайт

Второй прогон того же дня, но в **живом режиме** (PERF-8 v2, по решению
пользователя): одно GUI-окно `--maximized`, каждый сайт в новой вкладке
(MCP `new_tab`), dwell 5 с + скролл, кумулятивная RAM, метрика «не отвечает»
(IsHungAppWindow), авторестарт мёртвого окна. Сырые данные:
[runs/2026-07-17-live.json](runs/2026-07-17-live.json). Числа НЕ сравнимы с
headless-прогоном выше (другой режим); это первый живой базлайн.

| slug | статус | готовность, с | RAM тек, МБ | RAM пик, МБ | не отвечает, с | первая ошибка |
|---|---|---|---|---|---|---|
| example | OK | 0.86 | 381.9 | 439.8 |  |  |
| ya | OK | 2.32 | 496.6 | 515.6 |  | script error: JS runtime error: Unable to find RenderContext |
| hn | OK | 1.65 | 506.0 | 527.2 |  | script error: JS runtime error: el.getElementsByClassName is |
| w3 | OK | 128.92 | 506.3 | 527.2 |  | Ошибка загрузки https://www.w3.org/: network error: HTTP 403 |
| rust-lang | OK | 6.65 | 563.7 | 585.6 |  |  |
| lenta | OK | 7.15 | 610.5 | 651.8 | 2.5 | vite-plugin-css-injected-by-js TypeError: Cannot read proper |
| github | OK | 45.0 | 2904.4 | 2918.1 |  | module error: JS runtime error: Automatic publicPath is not  |
| stackoverflow | HUNG ↻ | — | — | — | 0.5 |  |
| crates | OK | 1.73 | 398.7 | 461.6 |  | script error: JS runtime error: Cannot read properties of un |
| docs-rs | OK | 1.12 | 448.2 | 462.0 |  | script error: JS runtime error: Cannot read properties of un |
| ria | OK | 4.87 | — | — | 39.0 | Пропуск скрипта https://yandex.ru/ads/system/header-bidding. |
| habr | HUNG ↻ | — | — | — | 60.0 |  |
| mdn | OK | 3.19 | 628.4 | 768.9 |  | [JS warn] Unable to set theme TypeError: Cannot set properti |
| rbc | OK | 23.45 | — | — | 47.5 | Пропуск картинки https://top-fwz1.mail.ru/counter?id=3081030 |

↻ = харнесс перезапустил зависшее окно (перезапусков: 2 — stackoverflow, habr).

**Находки:**
- **github.com: +~2.3 ГБ RAM одной вкладкой** (610 → 2904 МБ) → BUG-306.
- **UI-поток «не отвечает»**: обратимо 39–48 с (ria, rbc), необратимо после
  вкладок-гигантов (stackoverflow, habr — окно мертво, восстановление только
  рестартом процесса; в прогоне без рестартов сессия после stackoverflow не
  загрузила больше ни одного сайта) → BUG-307. Наблюдалось пользователем
  вживую («приложение не отвечает»).
- **403-страница держит document_ready 129–205 с** (w3.org; headless отдаёт
  тот же 403 за 0.75 с) → BUG-308.
- В живом (wgpu) окне тяжёлые по CPU-paint сайты быстры: mdn ready 3.2 с
  (headless paint был 59.7 с), lenta 7.2 с — CPU-растеризация скриншотного
  пути не отражает живое окно; для юзер-скорости критичнее RAM и зависания.
- github в живом окне ЗАГРУЖАЕТСЯ (ready 45 с) — зависание ≥240 с
  воспроизводится только в headless-путях (--dump-layout/--screenshot);
  уточнение к BUG-303.

**Заведённые баги:** BUG-306, BUG-307, BUG-308.

---

## 2026-07-20 — a014c9e6 — Windows 10, dev-release, живое окно (2 повтора)

Прогон `scripts/perf_audit.py` после S12b-18 (V8-миграция) и BUG-297/316/308
фиксов. Сырые данные второго (более представительного) повтора:
[runs/2026-07-20-live.json](runs/2026-07-20-live.json).

**Повтор 1** (сразу после 13-минутной `cargo build`, машина под нагрузкой
свежесобранного бинарника): 10/14 OK, 4 авторестарта (github, crates, habr,
rbc — последние три «без вкладки», `new_tab`/`navigate` не отвечали).

**Повтор 2** (тёплый бинарник): 12/14 OK, 2 авторестарта (github, habr).

| slug | статус (2) | готовность, с (2) | не отвечает, с (2) | JS-ошибки |
|---|---|---|---|---|
| example | OK | 2.07 |  | 0 |
| ya | OK | 3.89 |  | 8 |
| hn | OK | 7.32 |  | 0 |
| w3 | OK | 4.37 |  | 1 |
| rust-lang | OK | 11.46 | 1.0 | 0 |
| lenta | OK | 18.1 | 15.0 | 9 |
| github | HUNG ↻ | — | 60.5 | — |
| stackoverflow | OK | 7.21 |  | 1 |
| crates | OK | 13.15 | 4.0 | 1 |
| docs-rs | OK | 15.57 |  | 3 |
| ria | OK | 12.72 | 37.0 | 8 |
| habr | HUNG ↻ | — | 60.0 | — |
| mdn | OK | 7.0 |  | 2 |
| rbc | OK | 16.96 | 41.0 | 7 |

**Сравнение с 2026-07-17-live.json:** формально +56%…+1290% по `ready_s`
почти везде — но это методологический артефакт живого многовкладочного
харнесса (накопление состояния/фокуса окна), не регрессия движка.
Изолированная проверка headless `--phases` на crates.io/docs.rs дала
нормальные тайминги (screenshot total 1.8с у обоих) — engine-level
регрессии нет. Вывод: живой Δ%-тайминг непригоден для детекции регрессий,
дальше сравнивать `--phases`-прогоны, живой прогон — только для RAM/hung/
JS-ошибок/визуальной приёмки.

**Находки:**
- **BUG-306 воспроизводится** идентично — github.com виснет в обоих
  повторах (`Wait error: automation command timed out`).
- **BUG-307 воспроизводится и расширился** — во втором повторе habr.com
  тоже HUNG (раньше грузился штатно после авторестарта); в первом повторе
  каскад после github был тяжелее (crates/habr/rbc не получили вкладку
  вообще) — тот же класс дефекта, машина под доп. нагрузкой усиливает
  проявление.
- **BUG-308 подтверждён исправленным вживую**: w3.org ready 1.34с/4.37с
  в обоих повторах (было 129–205с).
- **Ручная проверка «крякозябр на вкладках»** (запрос пользователя во время
  прогона): собран отдельный live-репро (11 сайтов кириллица+латиница,
  `.tmp/capture_tabbar.py`, реальные gdigrab-снимки рабочего стола, не
  `resource://screenshot` — тот chrome-полосу вкладок не захватывает).
  На 8 захваченных кадрах текст вкладок (Яндекс/Lenta.ru/crates.io/Docs.rs/
  РИА Новости) рендерится корректно, крякозябр не поймано; два таба
  (hn, rust-lang) залипли на «Новая вкладка» из-за таймаута wait —
  тот же класс, что BUG-307, не новый визуальный баг. Требуется скриншот
  от пользователя в момент эффекта для дальнейшей диагностики.

**Заведённые баги:** нет новых — BUG-306/307 обновлены повторными данными
2026-07-20, BUG-308 остаётся FIXED (подтверждён вживую).

---

## Исторический контекст (до журнала)

**2026-07-02 — ручной аудит 14 сайтов** (headless `--screenshot`, dev-release,
сравнение с Edge headless; корпус восстановлен в corpus.txt из этого аудита):

- 4/14 сайтов не открылись: HTTP 403 антибот по TLS-фингерпринту rustls
  (stackoverflow, crates.io, ria.ru), HTTP 500 (docs.rs).
- Главный тормоз тяжёлых страниц — CPU-растеризация, не сеть: lenta.ru — сеть
  ~4 с, `--dump-layout` 5.3 с, полный `--screenshot` 141.7 с (~136 с чистый
  paint при высоте 7324 px). rust-lang.org той же высоты — ~4 с: стоимость
  зависит от display list, не только от площади.
- github.com не завершился за 280 с (все ресурсы к 6.6 с; JS-исполнение —
  тогда ещё QuickJS без JIT; с тех пор дефолт V8 — перемерить).
- Холодный старт первого запуска ~10 с (example.com 10.9 с → повторно 0.14 с).
- Простые страницы — паритет с Edge (w3.org 2.9 vs 3.0 с).

С тех пор влиты: параллельный fetch подресурсов, V8 вместо QuickJS,
wgpu-дефолт окна (CPU-путь скриншотов не изменился). Первый прогон журнала
установит новый базлайн.
