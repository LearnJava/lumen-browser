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

## 2026-09-23 — d530491e1 — Windows 10, dev-release (RP-10, повторный аудит против Edge, режим `compat`)

Первый прогон журнала в режиме `compat` (свежий процесс на каждый сайт — числа
сопоставимы между сайтами напрямую, в отличие от предыдущих `live`/`stability`
прогонов). Сырые данные: [runs/2026-09-23-compat.json](runs/2026-09-23-compat.json).

| slug | статус | готовность, с | RAM пик, МБ | CPU, с | не отвечает, с | JS-ошибки |
|---|---|---|---|---|---|---|
| example | OK | 2.47 | 619.9 | 6.8 | | 0 |
| ya | DEGRADED | 2.76 | 821.8 | 8.78 | | 5 |
| hn | OK | 8.15 | 705.1 | 8.88 | | 0 |
| w3 | SITE_REFUSED (403) | — | 495.2 | 0.84 | | 1 |
| rust-lang | OK | 4.49 | 717.7 | 8.06 | | 0 |
| lenta | DEGRADED | 18.76 | 963.5 | 141.09 | 142.0 | 20 |
| github | DEGRADED | 147.62 | 4347.9 | 291.14 | 250.0 | 2 |
| stackoverflow | TIMEOUT (403) | — | 523.2 | 2.65 | 7.5 | 1 |
| crates | BROKEN_RENDER | 20.11 | 521.4 | 4.01 | 8.5 | 1 |
| docs-rs | OK | 4.4 | 695.0 | 11.73 | | 0 |
| ria | HUNG ↻ | — | — | — | | 0 |
| habr | DEGRADED | 71.43 | 1312.0 | 48.58 | 63.0 | 18 |
| mdn | OK | 48.57 | 635.1 | 8.38 | 9.0 | 4 |
| rbc | NET_FAIL | — | 414.2 | 2.43 | 5.0 | 2 |

**Сравнение с 2026-07-20-live.json:** режимы разные (`live`/`stability` vs
`compat`), поэтому Δ% ниже — ориентир, не строгая регрессия (тот же вывод, что
в прогоне 2026-07-20 — живой Δ%-тайминг между разными харнессами не годится
для детекции регрессий). example +19%, ya -29%, hn +11%, rust-lang -61%,
lenta +4%, crates +53% ⚠ (но сменился статус: раньше 403, теперь рендерится),
docs-rs -72% (раньше 403/500-класс), mdn +594% ⚠ (см. находку ниже — не шум).

**Находки:**
- **corpus.txt обновлён** — список известных антибот-сайтов сдрейфовал:
  w3.org теперь 403 (было OK 2026-07-20, подтверждено дважды headless
  `--dump-source`, curl с обычным UA той же секундой отдаёт 200 — TLS-
  фингерпринт rustls, тот же класс, что stackoverflow/ria исторически);
  crates.io и docs.rs, наоборот, перестали быть 403/500 и теперь отдают 200
  (подтверждено headless).
- **BUG-306/BUG-683 переподтверждены, пик памяти вырос** — github.com теперь
  4341.7 МБ (было 2.9 ГБ на ревизии 2026-08-06), 250 с окно не отвечает;
  запись добавлена в файл бага.
- **ria.ru HUNG в живом окне, но headless (`--dump-source`/`--dump-layout`)
  отрабатывает штатно** — сошлось с уже известным [BUG-935](../../bugs/BUG-935-OPEN.md)
  (M4-путь rAF-DOM-мутаций не выполняется на дефолтной сборке, ria.ru
  явно упомянут в файле бага как репро-сайт с videojs-плеером) — не новый
  баг, реконфирмация.
- **BUG-1101 (новый)** — crates.io теперь проходит антибот, но рендерится
  пустой белой страницей: `[unhandled-rejection] TypeError: Cannot read
  properties of undefined (reading 'get')` сразу после двух `fetch()` к
  `api/v1/site_metadata`/`api/v1/summary`, воспроизведено headless.
- **BUG-1102 (новый)** — developer.mozilla.org: два skip-link-блока
  рендерятся по одной букве на строку (вертикальный столбец), специфично
  для этого сайта; отдельно паинт-стадия (`--phases`) — 104.48с, на порядок
  дороже соседних документационных сайтов (объясняет часть +594% Δ). Связь
  между визуальным дефектом и стоимостью паинта не подтверждена в этой
  сессии.
- **habr.com's file:// font — сайт-side дефект, не Lumen.** Два
  `link hint fetch failed: file:///home/web/sites/habr-web/releases/...`
  в stderr оказались буквальным `href="file:///home/web/sites/habr-web/…"`
  в живой HTML-разметке habr.com (проверено `curl -L`) — сайт сам отдаёт
  абсолютный путь ФС своего деплоя вместо CDN-URL шрифта. Не заводится.
- **rbc.ru NET_FAIL (DNS resolve) — не воспроизвелось повторно** headless
  `--dump-source` сразу после (200), классифицировано как сетевой шум
  этой сессии, не регрессия. Не заводится.
- **habr.com частично пустой рендер карточек ленты** (скриншот) —
  согласуется с уже перечисленными JS/сетевыми ошибками сайта
  (заблокированные easylist-скрипты, сорванный TLS-хендшейк, битые
  `file://`-шрифты) — не выделяется в отдельную находку без более
  прицельного разбора, какая из них ответственна за какую часть UI.

**Заведённые баги:** BUG-1101, BUG-1102. BUG-306/683 (github, ревизия
числами), BUG-935 (ria.ru, реконфирмация) обновлены без новых номеров.

---

## 2026-09-23 — c05801655 — Windows 10, dev-release — top100-foreign, трафик мимо VPN, против видимого Chrome 153

Сырые данные: [runs/2026-09-23-top100-split.json](runs/2026-09-23-top100-split.json) (Lumen, compat,
таймаут 240 с), [runs/2026-09-23-top100-split-chrome.json](runs/2026-09-23-top100-split-chrome.json)
(Chrome 153, видимое окно на весь экран, свежий профиль на сайт).

**Маршрут.** На машине включён VPN Hiddify (TUN `tun0`, sing-tun), весь трафик идёт через удалённый
прокси. A/B сырыми ClientHello шести клиентов (Chrome/Edge/Firefox/Lumen/curl/OpenSSL) на один IP
показал: TLS-рукопожатие через туннель — медиана 0.4-1 с с выбросами до 15 с у **всех** клиентов,
напрямую через Wi-Fi — ровные 47-63 мс у всех. Отпечаток TLS на скорость не влиял. Поэтому трафик
обоих браузеров шёл через локальный прокси мимо туннеля; 17 хостов, заблокированных напрямую
(youtube, facebook, instagram, x, linkedin, discord, bbc, quora, medium, …), прокси отправлял в туннель.

| Статус Lumen | Сайтов |
|---|---|
| OK | 18 |
| DEGRADED | 25 |
| BROKEN_RENDER | 23 |
| SITE_REFUSED | 19 |
| TIMEOUT | 7 |
| NET_FAIL | 7 |
| HUNG | 1 |

**Скорость** (сайты, открывшиеся в обоих браузерах; ready_s Lumen против `load` Chrome):

| Маршрут | Сайтов | Lumen, медиана | Chrome, медиана | Отношение, медиана |
|---|---|---|---|---|
| напрямую | 35 | 13.9 с | 5.2 с | 3.2× |
| через туннель | 11 | 42.4 с | 10.7 с | 3.9× |

**Сравнение с прошлым прогоном** (2026-09-23 днём, тот же коммит, через туннель): на 33 общих прямых
сайтах медиана Lumen 37.6 с → 14.0 с — около двух третей прежнего времени было VPN. Статусы с тем
прогоном не сравнимы: на прямом маршруте 27 сайтов из 34 неоткрывшихся не открылись и у Chrome
(403/пусто/таймаут — сайты отвечают по IP), это не дефекты Lumen.

**Находки:**
- Только 7 неоткрывшихся сайтов открылись у Chrome. Пять из них (zillow, reuters, accuweather, adobe,
  washingtonpost) — заголовки Chrome-профиля (`DNT: 1`, UA Chrome/130), установлено бисектом → BUG-1113.
  ebay и stackoverflow — челленджи Akamai/Cloudflare в теле 403, которое Lumen выбрасывает → BUG-1114.
  flipkart HUNG — один замер, не заводится до повтора (probe-method §9).
- Соединения: Lumen открыл 6685 TCP на 292 хоста (22.9 на хост), github.com — 98 соединений за одну
  загрузку. HTTP/2-пул отдаёт соединение одному запросу целиком → BUG-1115 / PERF-13.
- Окно «Не отвечает» ≥2 с на 20 сайтах: github 170 с, instagram 86, youtube 84, cnbc 82, flipkart 80,
  tradingview 65 — один прогон, github уже покрыт BUG-306/683/1108; новые баги не заводятся до повтора.
- Пиковая память: медиана 651 МБ, максимум 4.9 ГБ (github, BUG-683).

**Заведённые баги:** BUG-1113, BUG-1114, BUG-1115 (доработка → PERF-13).

---

## 2026-09-24 — разбор совместимости 48 сайтов top100 — Windows 10, dev-release, без блокировщика

Продолжение прогона 2026-09-23 выше: 48 сайтов, у которых отрисовка Lumen расходится с Chrome,
разобраны до конкретного API. Шесть параллельных агентов по 8 сайтов. На каждый сайт — видимое окно
`--maximized` против видимого Chrome 153, по одному окну за раз; инструмент —
`.tmp/compat/probe.py` в worktree аудита. Каждая находка сведена к маленькой локальной странице,
снятой в обоих браузерах.

**Всё перемерено без блокировщика** (`LUMEN_NO_ADBLOCK=1`, решение пользователя 2026-09-23).
Прогон 2026-09-23 шёл с включённым EasyList, и часть поломок была только его: spotify, whatsapp,
duolingo, imgur, discord. Строка `adblock: filter installed (N rules)` печатается при каждом старте и
о включённом блокировщике не говорит. Признак — `blocked: easylist`.

**Итог.** Заведено 28 багов, [BUG-1119](../../bugs/BUG-1119-FIXED.md)…[BUG-1146](../../bugs/BUG-1146-FIXED.md).
В 8 открытых дописаны сайты: BUG-892, 493, 568, 648, 863, 480, 970, 1114. Все отданы P6, очередь —
`STATUS-P6.md`, по числу сломанных сайтов.

| Причина | Сайты |
|---|---|
| нет `document.scripts`/`links` ([BUG-892](../../bugs/BUG-892-FIXED.md)) | imdb, espn, amazon (челлендж AWS WAF), discord |
| `document.referrer` — `undefined` ([BUG-1121](../../bugs/BUG-1121-FIXED.md)) | imgur, fandom, yahoo, yahoo-jp |
| `defer` исполняется в порядке документа ([BUG-1120](../../bugs/BUG-1120-FIXED.md)) | khanacademy, coursera |
| члены DOM не на прототипах интерфейсов ([BUG-1122](../../bugs/BUG-1122-FIXED.md)), `EventTarget` вне цепочки ([BUG-1123](../../bugs/BUG-1123-FIXED.md)), нет `CDATASection` (BUG-863) | youtube |
| `document.cookie` не сохраняется ([BUG-1119](../../bugs/BUG-1119-FIXED.md)) | msft-login |
| CSP nonce не пускает внешний скрипт ([BUG-1124](../../bugs/BUG-1124-FIXED.md)) | dropbox, gemini (гипотеза) |
| `<style>.sheet === null` сразу после вставки ([BUG-493](../../bugs/BUG-493-FIXED.md)) | twitch, quora, bbc |
| `document.write` не исполняет `<script>` ([BUG-568](../../bugs/BUG-568-FIXED.md)) | tumblr |
| `ShadowRoot` без `insertBefore` ([BUG-1130](../../bugs/BUG-1130-FIXED.md)) | archive |
| `classList` не итерируем ([BUG-1125](../../bugs/BUG-1125-FIXED.md)) | wordpress, mozilla |
| `blob:` URL не загружается ([BUG-1126](../../bugs/BUG-1126-FIXED.md)) | zoom, bing |
| `url()` во внешнем CSS от базы документа ([BUG-1127](../../bugs/BUG-1127-FIXED.md)) | apple, tumblr |
| `load` динамического скрипта после всей очереди ([BUG-1128](../../bugs/BUG-1128-FIXED.md)) | aliexpress (SystemJS) |
| `load` окна не ждёт вставленный скрипт ([BUG-1129](../../bugs/BUG-1129-FIXED.md)) | wordpress |
| прочие одиночные: `IntersectionObserverEntry`, `innerHTML` у `<script>`, `atob`, SVG с комментарием, `import.meta.resolve`, `getAttributeNames`, `History`, `HTMLDocument`, `postMessage` target, `srcset` с запятой, порядок XHR `progress`, `javaEnabled`, `BarProp`, `innerText` (BUG-1131…1144) | duolingo, bing, airbnb, tradingview, huggingface, samsung, whatsapp, yahoo-jp, webmd, amazon, apple, weibo |
| iframe `contentWindow`/`contentDocument` (BUG-480, BUG-970) | samsung, w3schools |
| `PerformanceObserver` buffered синхронно (BUG-648) | cnbc |
| 4xx/5xx заменяется страницей ошибки, `fetch` реджектит (BUG-1114) | reddit (403 и в Chrome), duolingo, fandom |

Служебные находки: [BUG-1145](../../bugs/BUG-1145-FIXED.md) (MCP `eval` отдаёт таймаут движкового
потока как «JS context not available»; мешал снять DOM на cnbc, gemini, udemy, imgur, github) и
[BUG-1146](../../bugs/BUG-1146-FIXED.md) (блокировщик игнорирует `$domain=`, виден только при
включённом блокировщике).

**Без движкового корня.** canva и character-ai почти на уровне Chrome. microsoft отдаёт Akamai
бот-стену (curl с UA Chrome получает её же). yahoo — сниффинг UA, BUG-1113. github — занятость
движкового потока, BUG-306. Не локализованы: soundcloud (`app.start()` без исключения), tiktok
(`RangeError: Maximum call stack size exceeded`), pinterest, walmart, naver (−150 узлов), udemy.
Сетевые гипотезы без репро: обрыв H2 без `close_notify` без повтора подресурса (espn, webmd) и
TLS-рукопожатие с `login.sina.com.cn` (weibo).

## 2026-09-24 — PERF-15, память ресурсов сайта — Windows 10, dev-release, живое окно, без блокировщика

База `bda865ff7` + ветка `p6-perf-15`. Два замера.

**Стенд `.tmp/seqlab`** (каждый ответ 700 мс, цепочка документ → скрипты → стили → картинки,
16 запросов). Последний ответ: первый визит 2281 мс (4 волны), повторные 1562 мс (2 волны,
13 ресурсов повторены заранее). Chrome 153 на том же стенде — 2.8 с. Приёмка «≤ 1.6 с» выполнена.

**top100, 16 сайтов** (spotify, duckduckgo, tumblr, google, mozilla, airbnb, naver, coursera,
huggingface, paypal, rakuten, aliexpress, apple, w3schools, wikihow, cnbc), `perf_audit.py --mode
compat` (свежий процесс на сайт), общий файл памяти `LUMEN_SITE_MEMORY_FILE`. Проходы R1, R2 —
запись (второй уже повторяет); затем выкл/вкл/выкл/вкл.

| проход | память | медиана ready_s | GET всего | повторено |
|---|---|---|---|---|
| R1 | запись | 7.70 | 1853 | 0 |
| R2 | вкл (повтор `last`) | 7.38 | 1848 | 405 |
| A | выкл | 7.67 | 1802 | 0 |
| B | вкл | 7.06 | 1873 | 369 |
| C | выкл | 7.99 | 1853 | 0 |
| D | вкл | 7.75 | 1840 | 400 |

Медиана средних по сайту: выкл 7.81 с, вкл 7.15 с; быстрее с памятью 9 сайтов из 16. Приёмка
«медиана повторного прохода ниже первого» выполнена, но разброс между проходами одного режима
(A 7.67 / C 7.99, B 7.06 / D 7.75) того же порядка, что и выигрыш: на таком корпусе эффект
виден, но не велик. Крупнее всего он на сайтах с длинной цепочкой открытия ресурсов
(duckduckgo 2.9 → 2.0 с, naver 6.2 → 4.1 с, mozilla 6.4 → 4.9 с, paypal 7.2 → 6.1 с);
на huggingface, cnbc и apple разброс внутри режима — десятки секунд, их вклад в медиану — шум.

**Что изменила первая версия замера.** Версия, повторявшая весь прошлый визит, на google,
paypal и naver давала +10…+82 лишних GET за визит: эти сайты меняют часть URL на каждом визите
(токены, хэши сборки, случайный набор картинок). Отсюда список `stable` — повторяется только то,
что загрузили два последних визита; вторая версия GET в сумме не прибавляет (1802–1853 выкл,
1840–1873 вкл).

**Статусы** не изменились, кроме google в проходе D (`DEGRADED`: `429 Too Many Requests` на
`/async/folif` — антибот Google после 11 визитов за час; в проходе R1 тот же 429 был без памяти).

## 2026-09-25 — PERF-13, одно HTTP/2-соединение на origin — Windows 10, dev-release, без блокировщика

A/B против `40ec51c04` (база ветки), оба бинарника с холодным профилем (`data/` стирается перед
каждым запуском — общий кэш и память ресурсов PERF-15 иначе подменяют сеть), трафик через VPN.

**Соединения** (40 с на загрузку, уникальные TCP процесса, опрос 20 мс):

| Сайт | База | PERF-13 |
|---|---|---|
| github.com | 93-120 соединений, 107-109 ответов | **6** соединений, 131/131 ответ |
| lenta.ru | 185 на 28 хостов (50 к одному) | 28 на 24 хоста (≤3 к одному) |

**Два латентных дефекта**, найденные первым A/B (на ветке без них github.com получал 22 ответа
из 109): HPACK-кодировщик без dynamic table size update → `GOAWAY(COMPRESSION_ERROR)` от Fastly
после ~45 запросов на соединение; окно соединения 65 535 байт на все потоки. Пока соединение
несло 1-2 запроса, ни один не проявлялся.

**ready_s** (compat, таймаут 90 с, 13 сайтов top100 — усечённый прогон, не полный корпус):
на ветке github, apple, spotify дошли до готовности (в базе HUNG), bing 33→8 с, canva 88→40 с,
microsoft 38→34 с. imdb 3-7→9-32 с — не регрессия: база остаётся на странице AWS WAF-челленджа
(повторный запрос `www.imdb.com` без ответа), ветка получает 200 и грузит настоящую страницу.
Остальное (reddit, etsy, nytimes, amazon) — в пределах разброса между повторами. Полный прогон
top100 против этого числа не делался.

---

## 2026-09-28 — 8bd5c5dd1 — Windows 10, dev-release — повтор top100 против 2026-09-23 и Chrome 153

Сырые данные: [runs/2026-09-28-top100-split.json](runs/2026-09-28-top100-split.json) (compat, таймаут
240 с, без блокировщика — `blocked: easylist` ни в одном логе, холодный `data/`). Chrome не перемерялся:
база — [runs/2026-09-23-top100-split-chrome.json](runs/2026-09-23-top100-split-chrome.json);
`chrome_major!()` = 153 совпадает с Chrome for Testing 153.0.8010.12 на машине.

**Маршрут — не тот же, что 09-23.** Прокси прогона 09-23 в репозитории не сохранился. Для этого прогона
написан [`scripts/split_proxy.py`](../../scripts/split_proxy.py): HTTP-прокси, исходящий сокет привязан к
Wi-Fi (мимо TUN), 41 домен из [split-tunnel-hosts.txt](split-tunnel-hosts.txt) и хосты, у которых прямой
connect не прошёл или прямое соединение закрылось без ответа, — в туннель (26 таких за прогон).
Lumen ходил в него через `proxy =` в `data/fingerprint.toml`: флаг `--proxy` не действует
([BUG-1210](../../bugs/BUG-1210-FIXED.md)). За HTTP-прокси Lumen теряет мультиплексирование HTTP/2 PERF-13
и получает `421` ([BUG-1209](../../bugs/BUG-1209-FIXED.md)), поэтому **скорость и соединения ниже —
Lumen-за-прокси, не прямой Lumen**; статусы от этого страдают только на сайтах с `421`.

| Статус | 09-23 | 09-28 |
|---|---|---|
| OK | 18 | 27 |
| DEGRADED | 25 | 31 |
| BROKEN_RENDER | 23 | 18 |
| SITE_REFUSED | 19 | 14 |
| TIMEOUT | 7 | 0 |
| NET_FAIL | 7 | 7 |
| HUNG | 1 | 3 |

**Переходы.** Лучше (30): tiktok, bing, espn BROKEN→DEGRADED; canva, quora, dropbox BROKEN→OK; yahoo-jp,
zoom, tumblr, webmd, huggingface, coursera, discord, github DEGRADED→OK; accuweather, ndtv
SITE_REFUSED→OK; nytimes, wsj, zillow, reuters SITE_REFUSED→DEGRADED; uber, target TIMEOUT→OK; costco
TIMEOUT→DEGRADED; temu, stackoverflow TIMEOUT→BROKEN; adobe NET_FAIL→BROKEN; mercadolivre, cricbuzz,
coinbase NET_FAIL→DEGRADED; flipkart HUNG→SITE_REFUSED. Хуже (14): live, bilibili, baidu, claude,
rakuten, xcom OK→DEGRADED; microsoft DEGRADED→BROKEN; fandom, walmart, cnbc, duolingo →NET_FAIL;
cnn, dailymail, udemy →HUNG.

**NET_FAIL — маршрут, не Lumen.** fandom, walmart, washingtonpost, nbcnews, uol падали
`H2 I/O: peer closed connection without sending TLS close_notify` на первом же документе. A/B
`--dump-source` 4 сайта × 4 раза: прямой путь прокси — 5 обрывов из 16, туннель — 0 из 16, тот же
прокси со всем трафиком в туннель — 0 из 16; сырой TLS из Python по Wi-Fi тоже рвётся (3 таймаута из 16).
Повтор этих сайтов: fandom OK, walmart/nbcnews/uol DEGRADED, washingtonpost/duolingo BROKEN_RENDER, cnbc
снова NET_FAIL (прямой путь режется после ClientHello, туннель — `TLS handshake over tunnel: EOF`).

**Скорость** (ready_s Lumen против `load` Chrome, сайты, открывшиеся в обоих и в оба прогона):

| Маршрут | Сайтов | Lumen 09-23 | Lumen 09-28 | Chrome | Отношение 09-23 | Отношение 09-28 |
|---|---|---|---|---|---|---|
| напрямую | 34 | 12.7 с | 17.8 с | 4.3 с | 3.4× | 2.8× |
| через туннель | 11 | 50.5 с | 10.2 с | 16.2 с | 3.2× | 0.9× |

Все 52 сайта, открывшиеся сейчас у обоих: Lumen 13.7 с, Chrome 5.7 с, 2.1×. «Туннельные» сайты у Lumen
в 5 раз быстрее — но Chrome мерился 09-23 на старом маршруте, а прямой путь сегодня медленнее
(TLS 0.1-0.6 с против 47-63 мс 09-23); отношения между днями не сравнимы строго.

**Память** (пик процесса): медиана 651 → 797 МБ, максимум 4.8 ГБ (github) → 3.0 ГБ (nytimes);
github 4.8 ГБ → 1.9 ГБ.

**Соединения.** Ожидания PERF-13 («около одного на origin») за прокси не проверяемы: 8944 `CONNECT` на
824 пары сайт/хост — 10.9 на хост (09-23 без прокси: 22.9 TCP на хост); nytimes `www.nytimes.com` 478,
x.com `abs.twimg.com` 454, github `github.githubassets.com` 130. Причина — BUG-1209.

**Наблюдение.**
- «Не отвечает» ≥ 2 с: 28 сайтов (09-23 — 20): nytimes 211 с, coinbase 92, yahoo 75, tradingview 64,
  reddit 62, ndtv 48, tumblr 32, archive 28, adobe 24, discord 22, wsj 21, airbnb 20, …; github 170 → 13 с,
  instagram 86 → 0, youtube 84 → 0. tradingview 64 с и archive, airbnb, discord, linkedin, zoom — второй
  прогон подряд; github — BUG-306.
- `H2 stream … timed out after 60s` (BUG-1205): 2 сайта (airbnb ×3, reuters ×2) в основном прогоне,
  dailymail ×2 в повторе.
- `EvalError: Code generation…` (BUG-1206): 3 сайта — youtube, live ×12, gemini ×4.
- Белый кадр среди OK: instagram (`frame_dominant_frac` 0.997, ноль ошибок).
- Повторившиеся HUNG: cnn, dailymail, udemy — оба прогона, cnn и udemy headless через туннель тоже
  → [BUG-1211](../../bugs/BUG-1211-FIXED.md).

**Сверка с закрытыми багами** (сайт: 09-23 → 09-28):

| Подтвердилось | Не подтвердилось |
|---|---|
| accuweather SITE_REFUSED→OK, zillow →DEGRADED, adobe NET_FAIL→BROKEN (заголовки прошли, 200) — BUG-1113 | washingtonpost — NET_FAIL маршрута; в повторе 200 и BROKEN_RENDER |
| stackoverflow TIMEOUT→BROKEN: тело 403 теперь отдаётся, челлендж Cloudflare стартует — BUG-1114 | ebay — 403 и у curl с UA Chrome 153 по Wi-Fi (IP), не Lumen |
| dropbox, quora, zoom, tumblr, webmd, huggingface, coursera, yahoo-jp, discord →OK | khanacademy BROKEN, узлов 31→417 (`__KA_DATA__ not found` ушла), кадр — спиннер |
| youtube: ошибки BUG-1122/1123 ушли, узлов 428→1265 | youtube BROKEN — теперь BUG-1207 и BUG-1206 |
| archive: `t.insertBefore` ушла (BUG-1130), samsung: `getAttributeNames` ушла (BUG-1136) | archive, samsung — BROKEN (samsung: `this.videoInfo.el.load is not a function`) |
| apple (`javaEnabled`), wordpress (`classList.values`), whatsapp (`requireLazy`), airbnb (`atob`) — исходные ошибки ушли | статус DEGRADED — другими ошибками |
| | imdb, amazon — AWS WAF: `AwsWafIntegration is not defined`, челлендж не проходит (BUG-1179) |
| | twitch BROKEN — скрипты `assets.twitch.tv` получают `421` (BUG-1209); BUG-493 до исправления BUG-1209 не проверить |

Новых регрессий закрытых багов (та же ошибка снова) не найдено — баги на закрытые не заводились. Сведение
каждого BROKEN/DEGRADED к локальной странице в этот прогон не вошло: сначала нужен BUG-1209, иначе часть
поломок — `421` прокси, а не движок.

**Заведённые баги:** BUG-1209, BUG-1210, BUG-1211.

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

## 2026-10-06 — база отзывчивости Chromium 153 (без Lumen) — Windows 10, без VPN

Скрипт [`scripts/chrome_interaction_baseline.py`](../../scripts/chrome_interaction_baseline.py),
сырые данные — [runs/2026-10-06-chrome-interaction.json](runs/2026-10-06-chrome-interaction.json).
Портабельный Chromium 153.0.8010.12 из кэша Playwright, свежий профиль на прогон, `--start-maximized`,
сценарий census BUG-935: загрузка → 15 с простоя → 20 щелчков колеса по 400 px через 200 мс
(синтетический `Input.dispatchMouseEvent`, не колесо ОС). Метрики — из CDP-трейса главного потока
рендерера; медиана по прогонам, 3 прогона на сайт.

| сайт | стили p90, мс | раскладка p90 / max, мс | длинные задачи при прокрутке, TBT мс | колесо→кадр p50 / p90, мс | кадры целиком / частично / сброшено |
|---|---|---|---|---|---|
| ria.ru | 1,45 | 0,72 / 1,1 | 0 | 18,5 / 64 | 117 / 7 / 7 |
| lenta.ru (2 прогона) | 0,54 | 0,59 / 17,4 | 18 | 63,8 / 178 | 436 / 24 / 77 |
| rbc.ru | 0,62 | 1,61 / 5,9 | 132 | 63,8 / 129 | 769 / 73 / 43 |

**Находки:**
- Пересчёт стилей и раскладка у Chromium — доли миллисекунды на проход (p90 ≤ 1,6 мс), за 6 с
  прокрутки суммарно 10–200 мс. Это ориентир для флашей Lumen (медиана 4,8 мс на ria.ru после BUG-935 S91).
- Задержка «колесо → кадр со сдвигом» у Chromium на синтетическом вводе не один кадр: p50 18–64 мс,
  на lenta.ru сбрасывается ~14 % кадров. Плавность «на глаз» этим не измерена — для этого THREAD-5
  (настоящее колесо через `SendInput`, доля кадров вовремя).
- lenta.ru в одном прогоне из трёх увела на авторизацию `id.sber.ru` — прогон отброшен, скрипт теперь
  отбрасывает смену хоста сам. У rbc.ru `load` то 0,5 с, то не наступает за 60 с (рекламные iframe) —
  это свойство сайта, простой и прокрутка начинаются после ожидания в обоих случаях.

**Сравнение с Lumen:** не проводилось — следующий шаг THREAD-5 (очередь P6).
**Заведённые баги:** нет новых.

## 2026-10-06 — THREAD-5: плавность прокрутки колесом, Lumen против Chromium 153 — Windows 10, без VPN

Инструмент: [`scripts/scroll_smoothness_run.py`](../../scripts/scroll_smoothness_run.py) (ввод `SendInput`, окно на переднем плане, `--maximized`, `LUMEN_NO_ADBLOCK=1`) и [`scripts/scroll_smoothness.py`](../../scripts/scroll_smoothness.py) (метрики по журналу `LUMEN_PRESENT_LOG`, [`present_log.rs`](../../crates/shell/src/present_log.rs)). Сценарий: загрузка 20 с + простой 5 с, 5 серий по 6 щелчков через 40 мс, пауза 800 мс; 3 прогона на сайт. Дисплей 1920×1080, 60,027 Гц, период 16,66 мс. Сырые данные — [runs/2026-10-06-scroll-smoothness.json](runs/2026-10-06-scroll-smoothness.json). Медианы по прогонам:

| сайт | браузер | кадров | в срок | макс. разрыв, мс | рывков (>2 периодов) | задержка щелчок→кадр, мс |
|---|---|---|---|---|---|---|
| ria.ru | Lumen | 65 | 0,92 | 44 | 2 | 11 |
| ria.ru | Chromium | 86 | 0,96 | 68 | 1 | 52 |
| lenta.ru | Lumen | 30 | **0,00** | 51 | **22** | 8 |
| lenta.ru | Chromium | 152 | 0,96 | 100 | 6 | 38 |
| rbc.ru | Lumen | 147 | 0,95 | 23 | 0 | 3 |
| rbc.ru | Chromium | 329 | 0,98 | 50 | 2 | 28 |

**Находки:**
- lenta.ru — главный разрыв. Lumen даёт ровно один кадр на щелчок (30 кадров на 30 щелчков, интервал ≈ 40 мс = шаг ввода): между щелчками прокрутка не продолжается, поэтому «в срок» 0 % и 22 рывка. Chromium на тех же щелчках рисует кадр каждые 16,66 мс (плавная прокрутка по таймеру компоновщика).
- ria.ru и rbc.ru: Lumen по метрике «в срок» почти не хуже, но кадров вдвое меньше — часть интервалов в «серии» заполнена не прокруткой, а кадрами страницы; метрика «в срок» без числа кадров на щелчок завышает Lumen. Для порогов нужна пара: доля в срок **и** кадров на серию.
- Задержка Lumen ниже (3–11 мс против 28–52 у Chromium) — но Chromium меряется `EventLatency` до кадра со сдвигом, а Lumen — до любого present после щелчка; числа сравнимы лишь по порядку.
- Максимальный разрыв Chromium (до 100 мс) — единичные паузы на тяжёлой странице, не системные.

**Пороги «не хуже Chromium» для THREAD-6…THREAD-12** (на тех же сайтах и сценарии): доля в срок ≥ 0,95; рывков ≤ 6 на прогон; кадров на серию из 6 щелчков не меньше половины числа у Chromium (ria ≥ 43, lenta ≥ 76, rbc ≥ 164 за прогон из 5 серий); для lenta.ru — кадр каждые ≤ 2 периода в течение прокрутки, то есть сначала добиться, чтобы прокрутка продолжалась между щелчками.
**Оговорка:** замер ведёт `SendInput` на живом дисплее — руками мышь и окно во время прогона не трогать; три прогона дают разброс (кадров у Chromium на lenta.ru 86…263), пороги брать по медиане.
**Заведённые баги:** нет новых.

## 2026-10-06 — THREAD-6: поправка к базе THREAD-5 по lenta.ru

Строка lenta.ru в таблице THREAD-5 для Lumen измеряла **не прокрутку**: `body{overflow-y:scroll;overflow-x:hidden}` оставался в `scroll_containers` контейнером размером с документ (1920×7980), `try_scroll_overflow_container` перехватывал каждый щелчок (двигал его `scroll_y` и сбрасывал при пересборке), страница стояла на `scroll_y 0`, «кадр на щелчок» — перерисовка без сдвига. Исправлено: `lumen_layout::collect_page_scroll_containers` исключает `html` и (при `visible` у `html`) `body` — их `overflow` относится к вьюпорту (CSS Overflow L3 §3.5).

После правки на lenta.ru страница реально прокручивается, и открылось настоящее узкое место: кадры UI-потока 160–314 мс (`LUMEN_FRAME_LOG=1`), часть щелчков доходит до окна с секундными задержками (в одном прогоне из 30 щелчков обработано 9–14). Прогон `scroll_smoothness_run.py lumen https://lenta.ru`: кадров 22–113, «в срок» 0,33–0,42, рывков 2–12 — до порогов THREAD-5 (≥ 0,95) далеко. Это работа THREAD-7/10/11, база для них — эти числа, а не прежние «30 кадров / 0,00». ria.ru и rbc.ru не менялись (ria 0,05–0,16 в срок, rbc 0,92–0,94).
`SS_STDERR=<файл>` в `scroll_smoothness_run.py` пишет stderr Lumen (для `LUMEN_FRAME_LOG`).

## 2026-10-06 — THREAD-6 срез 4: шаг self-tick рендер-потока

`scroll_smoothness_run.py lumen https://lenta.ru` с `LUMEN_FRAME_LOG=1`: UI-кадры во время прокрутки 1–3 мс, но периодически 54–80 мс целиком в фазе `js` (rAF, SMIL-тик `smil.rs:28`, рестайл `relayout.rs:1512`, задачи движка 11–35 мс). Колесо обрабатывается за миллисекунды — пачка событий winit (вариант 2) ничего бы не дала, очередь колеса не стоит.

Журнал present показал второй дефект: тики рендер-потока шли с шагом ~30 мс. Причина — `recv_timeout(MOMENTUM_TICK)` отсчитывался от конца прошлого `render`, а тот блокируется на vsync (≈ период) → период + период. Теперь таймаут = `MOMENTUM_TICK − время с начала прошлой работы`.

| lenta.ru, 3 прогона | кадров | в срок | макс. разрыв, мс | рывков |
|---|---|---|---|---|
| до (1 прогон) | 110 | 0,15 | 93 | 18 |
| после | 145 / 143 / 87 | 0,43 / 0,42 / 0,50 | 64 / 98 / 76 | 12 / 8 / 6 |

До порога 0,95 далеко: остаток — стопоры UI в фазе `js` (THREAD-7/10/11). Замечено: из 30 щелчков до окна доходит 13–19 событий колеса (`W` в журнале) — причину не выясняли.

**Потеря колёсных событий (13–19 из 30) — не потеря.** Windows склеивает колёсные сообщения в очереди потока, пока тот не качает её (UI в долгом кадре js 54–80 мс): `W -4` в журнале = четыре щелчка. Сумма |dy| по прогону = 30, дистанция прокрутки цела. Курсор и окно проверены (`WindowFromPoint` = окно Lumen на всех 30 щелчках). `scroll_smoothness_run.py` теперь выводит `wheel_clicks` (щелчки) рядом с `wheel_events` (сообщения) и ставит курсор в центр перед каждым щелчком. Следствие для плавности: после стопора дельты приходят одним скачком — кривая щелчка стартует с суммарным сдвигом, и это ещё один довод убрать стопоры UI (THREAD-7/10/11), а не чинить ввод.

## 2026-10-06 — THREAD-6 срез 6: кривая щелчка главнее `scroll_y` кадра UI

`process_batch` рисовал кадр UI со значением `scroll_y`, снятым на UI-стороне, даже если кривая щелчка в рендер-потоке уже ушла вперёд (UI встал в долгий кадр) — страница дёргалась назад. Теперь пока кривая активна, `frame_scroll_y` берёт её значение по часам рендер-потока. Юнит-тест есть, живой прогон `scroll_smoothness_run.py` не делался (нужен передний план). Остаток: ввод колеса и событие `scroll` всё ещё идут через UI-поток.

## 2026-10-06 — THREAD-6 срез 7: живой замер среза 6

`scroll_smoothness_run.py lumen`, `dev-release` на 60bce9581, `LUMEN_NO_ADBLOCK=1`, 3 прогона (30 щелчков, 5 серий):

| сайт | кадров | в срок | макс. разрыв, мс | рывков |
|---|---|---|---|---|
| lenta.ru | 132 / 134 / 137 | 0,41 / 0,41 / 0,41 | 81 / 81 / 63 | 11 / 14 / 7 |
| ria.ru | 122 / 52 / 102 | 0,30 / 0,64 / 0,38 | 56 / 45 / 63 | 8 / 2 / 7 |

Срез 6 «в срок» не сдвинул (lenta 0,42–0,50 → 0,41): кривая щелчка теперь не откатывается назад, но кадры по-прежнему рвутся стопорами UI-потока в фазе `js` (THREAD-7/10/11). Пороги THREAD-5 (≥ 0,95) не достигнуты; код в этом срезе не менялся.

## 2026-10-06 — THREAD-7 срез 1: apply_relayout_result без глубоких копий стилей

`LUMEN_FRAME_LOG=1`, ria.ru, `dev-release`. Шаги `apply-step` на UI-потоке, сумма за прогон (6–7 тиков):

| шаг | до, мс | после, мс |
|---|---|---|
| `transitions_sync` | 118,6 | 5,9 |
| `dl_splice_diff_cache` | 109,0 | 0,2 |

Причины: `collect_box_styles` глубоко копировал каждый `ComputedStyle` (теперь `Arc`); `tile_grid.update_from_diff` и клон DL в `display_list_cache` не имеют читателей. `apply_ms` одного инкрементального тика 66,5→7,6. Живой замер плавности в этом прогоне шумный (мышь, 10–22 щелчка из 30) — вывод по THREAD-5 не делается; цель ≤ 1 мс не достигнута (≈2,8 мс на тик).

## 2026-10-06 — THREAD-7 срез 2: замер остатка apply на UI и IdMap для prev_styles

`LUMEN_FRAME_LOG=1`, lenta.ru (59 тиков), `dev-release`. Шаги `apply-step` на UI-потоке, среднее за тик: `transitions_sync` 1,02, `clone_hit_test_tree` 0,75, `cv_snap_scroll_state` 0,47, `js_shift_collect` 0,41, `frame_sync` 0,16; сумма ≈2,8 мс, `apply_ms` медиана 4,2 (макс 8,9). Шаги `collect_*` (8 мс на `collect_computed_styles`) идут в отложенной задаче движка, UI не блокируют.

Правка: `prev_styles`/`chrome_prev_styles` — `IdMap` вместо SipHash-`HashMap` (`NodeId` — плотный индекс). `transitions_sync` 1,02→0,79 мс, `apply_ms` медиана 4,2→3,9. Плавность (lenta «в срок» 0,25, ria 0,26–0,34) не изменилась: стопоры кадра UI 55–80 мс — не `apply`. Критерий ≤ 1 мс не достигнут: остаток упирается в `clone_hit_test_tree` и обходы дерева, снять их можно только через `Arc<LayoutBox>`, а `scrolling.rs:83` мутирует дерево на месте (общий `Arc` сделает каждый скролл глубокой копией).

## 2026-10-06 — THREAD-10: A/B рестайла M4 — движковый путь (`LUMEN_BUG935_M4_SWAP=0`) против on-thread Guarded (дефолт S85)

Код после THREAD-7/9 (main `de3b82321`), `dev-release`, `scroll_smoothness_run.py lumen`, 3 прогона на сайт, без VPN, `--maximized`. Сырьё: [`runs/2026-10-06-thread10-m4-swap-off.json`](runs/2026-10-06-thread10-m4-swap-off.json), [`runs/2026-10-06-thread10-m4-guarded.json`](runs/2026-10-06-thread10-m4-guarded.json).

| сайт | режим | «в срок» по прогонам | рывков | макс. разрыв, мс | задержка щелчок→кадр, медиана, мс |
|---|---|---|---|---|---|
| lenta.ru | движок (`=0`) | 0,62 / 0,69 / 0,78 | 9 / 5 / 4 | 66–99 | 8,7 / 20,1 / 6,9 |
| lenta.ru | Guarded (дефолт) | 0,51 / 0,63 / 0,50 | 17 / 6 / 9 | 73–97 | 6,8 / 23,0 / 6,9 |
| ria.ru | движок | 0,31 / 0,36 / 0,33 | 12 / 7 / 9 | 71–93 | 12,1 / 14,9 / 12,4 |
| ria.ru | Guarded | 0,41 / 0,40 / 0,74 | 7 / 6 / 3 | 36–77 | 15,4 / 13,0 / 12,8 |
| rbc.ru | оба | 0,97–1,00 | 0 | 17–23 | 1,3–3,4 |

Вывод: на lenta.ru движковый путь **не хуже** и по медиане лучше (в срок 0,69 против 0,51; рывков 5 против 9); на ria.ru Guarded чуть лучше (0,40 против 0,33), rbc.ru не различает. Разброс между прогонами сопоставим с разницей (обработано 11–23 щелчка из 30, Guarded lenta run 1 — задержка 1065 мс), поэтому однозначного победителя нет. Ни один режим не достиг порога THREAD-5 (≥ 0,95) на lenta/ria. Дефолт не менялся; решение — за пользователем.

## 2026-10-06 — THREAD-13 срез 6: замер плавности колеса, Lumen против Chromium 153 — Windows 10, без VPN

Инструмент и сценарий — как THREAD-5 (`scroll_smoothness_run.py`, `SendInput`, `--maximized`, 8 прогонов на сайт, дисплей 60,027 Гц), бинарник — `main` a3ef2805a (после срезов 1–5, поток браузера включён по умолчанию). Сырые данные — [Lumen](runs/2026-10-06-thread13-s6-lumen.json), [Chromium](runs/2026-10-06-thread13-s6-chromium.json). Значения — медианы по 8 прогонам.

| сайт | браузер | кадров | в срок (медиана / мин.) | рывков (медиана / макс.) | макс. разрыв, мс | задержка, мс |
|---|---|---|---|---|---|---|
| ria.ru | Lumen | 150 | 0,92 / 0,86 | 3 / 5 | 56 | 10,0 |
| ria.ru | Chromium | 92 | 0,95 / 0,89 | 2,5 / 6 | 83 | 42,6 |
| lenta.ru | Lumen | 158 | 0,89 / 0,88 | 3 / 5 | 93 | 5,2 |
| lenta.ru | Chromium | 129 | 0,94 / 0,91 | 5 / 11 | 100 | 25,4 |
| rbc.ru | Lumen | 154 | 0,93 / 0,88 | 0 / 2 | 31 | 1,3 |
| rbc.ru | Chromium | 338 | 0,99 / 0,92 | 1,5 / 16 | 50 | 25,0 |

Против порогов THREAD-5: рывки ≤ 6 — выполнено на всех трёх (медиана и максимум); кадров не меньше половины Chromium — выполнено на ria и lenta (больше Chromium), на rbc.ru нет: 154 против 169 (половина от 338); задержка ниже, чем у Chromium. **Доля в срок ≥ 0,95 не выполнена ни на одном сайте** (0,92 / 0,89 / 0,93) и хуже Chromium на 0,03–0,06; у Chromium порог тоже не везде достигается (lenta 0,94).

Вывод: по рывкам и разрывам Lumen теперь не хуже Chromium (lenta.ru: 0,00 и 22 рывка в THREAD-5 → 0,89 и 3), но критерий ADR-032 «не хуже Chromium» по доле в срок не выполнен. Дефолт не менялся, `LUMEN_NO_BROWSER_THREAD` не удалялся, ADR-032 Future не закрыт; простой CPU (инвариант 6) в этом прогоне не мерился. Остаток — выровнять интервалы present (в Lumen ~5–10 % интервалов длиннее порога при непрерывной прокрутке).

### Дополнение к срезу 6 (2026-10-06): интервалы present и простой CPU

**Интервалы.** Временная проба (длительность `render()` рядом с меткой present, в коммит не вошла), ria.ru, 2 прогона × 30 щелчков, `main` a3ef2805a. Интервалы длиннее 17,7 мс между тиками рендер-потока (18–36 мс) равны длительности самого `render()` в пределах 0,3 мс: тик не опаздывает, а `render()` (растр кадра + ожидание vsync) занимает 1,1–2,1 периода. Планировщик тиков (`OWNED_MIN_TICK_MS`, `recv_timeout`) долю в срок не портит — «выравнивать интервалы present» таймером нечем. Остальные длинные интервалы — кадры потока браузера с новым списком (80–100 мс, отдельный полный растр). Доля в срок в этих прогонах 0,90–0,96 (медиана интервала 16,4 мс) — разброс между прогонами сопоставим с разницей с Chromium (0,95). Причина — стоимость растра отдельных кадров, то есть THREAD-11 (тайлы с упреждением), а не темп тиков.

**Простой CPU (инвариант 6).** `TotalProcessorTime` за 20 с после 30 с загрузки, без ввода:

| страница | поток браузера | CPU за 20 с |
|---|---|---|
| about:blank | вкл | 0,11 с |
| about:blank | `LUMEN_NO_BROWSER_THREAD=1` | 0,02 с |
| ria.ru | вкл | 10,3 с |
| ria.ru | `LUMEN_NO_BROWSER_THREAD=1` | 13,0 с |

Поток браузера простоя не добавляет (на ria.ru с ним не больше, чем без); пустая страница — ~0,5 % ядра. Но ria.ru без ввода жжёт ~0,5 ядра сама по себе (таймеры/анимации страницы) в обоих режимах — это не регрессия среза, а отдельная проблема простоя на живых страницах; в очередь не заводилась.

Критерий дефолта (доля в срок ≥ Chromium) по-прежнему не выполнен; дефолт не менялся.

## 2026-10-06 — THREAD-11, проба перед кодом: откуда длинные кадры при прокрутке на несколько экранов — lenta.ru, Windows 10, без VPN

`scroll_smoothness_run.py lumen https://lenta.ru --bursts 3 --ticks 40 --tick-ms 30` (120 щелчков, несколько экранов подряд), `main` e48820de6, Vulkan/Intel Iris Plus, `LUMEN_FRAME_LOG=2`. 3 прогона: доля в срок 0,97 / 0,91 / 0,87, рывков 3 / 3 / 5, максимальный разрыв 88–94 мс.

- Длинные интервалы (60–130 мс) — **все между `frame`-презентами** (кадры потока браузера с новым списком отрисовки), не между тиками колеса. Тики рендер-потока в длинных интервалах не участвуют.
- В журнале кадров прогона с `LUMEN_FRAME_LOG=2`: 35 классификаций «band repaint» (мерка `ScrollCache::plan`, только измерение), из них **34 — `delta ContentChanged`** (хэш содержимого изменился: страница перерисовывается сама — ленивые картинки, анимации), 1 — первый кадр. Выхода прокрутки за полосу как причины перерисовки в этом прогоне нет: 467 из 634 кадров — на `scroll_y` 0–1000, полоса попадает.
- Следствие для THREAD-11: тайлы «с упреждением по направлению прокрутки» на этом сценарии не лечат ничего — полоса не промахивается. Выигрыш даёт только **частичная инвалидация**: изменение содержимого перерисовывает затронутые тайлы, а не всю полосу (сейчас хэш содержимого целиком → полный растр полосы, 60–130 мс на Intel Iris). Это другая постановка, чем в строке ROADMAP.

## 2026-10-06 — THREAD-11 срез 6: A/B `LUMEN_BAND_PARTIAL` 0/1 — lenta/ria/rbc, Windows 10, без VPN

Сценарий пробы (`scroll_smoothness_run.py --runs 3 --bursts 3 --ticks 40 --tick-ms 30`, `LUMEN_NO_ADBLOCK=1`), `main` bbfeb3383 (срезы 1–5), `dev-release`. Прогоны одиночные по 3 на ячейку, разброс между ними больше разницы.

| сайт | `=0` в срок | `=0` макс. разрыв, мс | `=1` в срок | `=1` макс. разрыв, мс |
|---|---|---|---|---|
| lenta.ru | 0,93 / 0,89 / 0,85 | 94 / 86 / 86 | 0,87 / 0,86 / 0,85 | 87 / 89 / 98 |
| ria.ru | 0,91 / 0,84 / 0,93 | 49 / 71 / 62 | 0,94 / 0,72 / 0,87 | 74 / 85 / 96 |
| rbc.ru | 0,88 / 0,94 / 0,99 | 70 / 32 / 20 | 0,89 / 0,86 / 1,00 | 90 / 32 / 19 |

Вывод: выигрыша нет, `=1` в среднем не лучше. Причина видна в журнале кадров (`LUMEN_FRAME_LOG=2`, lenta, 120 щелчков): за прогон один вердикт `band-partial: adopt`, ни одного `strips`; `page-compose MISS` — 5, все на смещении полосы при прокрутке (растр полосы 10–28 мс). Кадры 150–350 мс в логе — это `build: chrome` (150–350 мс на UI-потоке, THREAD-12), а не растр полосы. Частичной инвалидации в этом сценарии почти нечего лечить: «34 `ContentChanged`» пробы — классификация мерки `ScrollCache::plan`, а не промахи полосы. Дефолт **не переворачивался**; критерий THREAD-11 (разрыв > 60 мс от смены содержимого) на этом сценарии не воспроизводится, остаточные разрывы ≥ 60 мс — из THREAD-12 и растра при сдвиге полосы. S7 (растр-воркеры) по этим данным не обоснован.

## 2026-10-06 — THREAD-12: корень `build: chrome` — мьютекс документа в `doc_selection_overlay` — Windows 10, без VPN

Проба по фазам (временные метки внутри шага 6, не вошли в коммит), ria.ru, `scroll_smoothness_run.py`, `LUMEN_FRAME_LOG=1`. Всё время `build: chrome` (500–980 мс в кадрах с пиками) приходилось на блок выбора overlay страницы до раскладки хрома; сам хром (`chrome_overlay_segment`) — 0,05–0,13 мс. В блоке `doc_selection_overlay` делал `document.lock()` на UI-потоке, пока поток движка держал документ в JS-задаче. Правка: `try_lock`, при занятом мьютексе — результат прошлого кадра для того же документа.

После правки (1 прогон, 2 серии × 30 щелчков): `build: chrome` медиана / p95 / макс., мс — ria.ru 0,34 / 1,16 / 1,16 (17 кадров); lenta.ru 0,23 / 0,47 / 12,6 (381); rbc.ru 0,28 / 1,10 / 22,3 (96). До: ria.ru 26–981 мс в кадрах с пиками. Критерий ≤ 2 мс выполнен по медиане и p95; остались единичные выбросы 5–22 мс (5 из 494 кадров), вне этого блока (метка блока ≤ 0,03 мс) — вероятно, начало шага 5 на первых кадрах страницы, не исследовалось.

### Дополнение к THREAD-12 (2026-10-07): выбросы 5–22 мс — артефакт пробы

Выбросы `build: chrome` 5–22 мс после правки появлялись в прогонах, где временная проба печатала `eprintln!` ВНУТРИ измеряемого окна: stderr делит блокировку с потоком движка (он пишет большие блоки журнала), и печать пробы ждала её. Метки поставлены в переменные, печать вынесена за `bmarks[0]`: 3 прогона lenta.ru + 3 ria.ru (`LUMEN_FRAME_LOG=1`; число кадров не фиксировалось, в последних прогонах их было мало — 2 и 14, так что выборка скромная) дали один кадр выше 2 мс — 8 мс на ria.ru, из них 7,6 мс между началом блока overlay и концом сборки хрома (метка 2,45 → 10,10), причина не выясняется (одиночный выброс). Корректная проба внутри окна кадра — только переменные, печать после окна.

## 2026-10-13 — COMPAT-AB-IDENTITY-HARNESS — плечо Chrome и матрица заголовков (не прогон корпуса)

Приборы для A/B идентичности: `scripts/compat_ab_chrome.py` (Chrome for Testing 153.0.8010.52, `plain`/`lumen`),
`scripts/header_matrix.py` (TLS × заголовки, бисект, снятие наборов), `scripts/header_echo_server.py`,
`scripts/wall_signatures.py`. Маршрут: напрямую, одна машина, один IP. Сырьё — `runs/2026-10-13-ab-chrome.json`,
`runs/2026-10-13-header-matrix.md`, `runs/2026-10-13-header-bisect.md`; наборы заголовков — `header-sets/`.

- **Таблица BUG-1113 воспроизведена частично, остальное — смена сайтов.** zillow: 403 PerimeterX с набором
  `lumen-2026-09-23` на обоих TLS, 200 с набором Chrome 153; бисект: решает один заголовок, `User-Agent`
  (Chrome/130 → 153). Решают заголовки, не TLS, как и в BUG-1113. accuweather, adobe, washingtonpost теперь 200 и с
  `DNT: 1` — Akamai-правило по `DNT` на этом IP не срабатывает. reuters: 401 DataDome во всех ячейках и у
  настоящего Chrome (репутация IP, не заголовки).
- **khanacademy с Chrome/130.** Настоящий Chrome: `WALL_SOFT` (200, `unsupported-browser`), с 153 — `OK`, оба
  варианта идентичности. `curl_cffi` с Chrome/130: 200 `unsupported-browser`; на не-браузерном TLS сайт сейчас отдаёт
  «Client Challenge» (`/_fs-ch-…`) при любом UA — это отдельная подпись `js-challenge`.
- **Токен `Lumen/x.y` на 6 сайтах:** блокировок из-за токена 0 из 6 (`runs/2026-10-13-ab-chrome.json`). Выборка
  мала, решение Р1 требует ≥100 сайтов (COMPAT-AB-IDENTITY).
- **Расхождения набора Lumen (`lumen-chrome_unbranded`) с Chrome 153 на проводе:** у Lumen есть
  `Cache-Control: max-age=0` (Chrome на обычной навигации его не шлёт), нет `Upgrade-Insecure-Requests: 1`,
  `Sec-Fetch-User: ?1`, `zstd` в `Accept-Encoding`; порядок другой (Chrome: `sec-ch-ua*`, `Upgrade-Insecure-Requests`,
  `User-Agent`, `Accept`, `Sec-Fetch-*`, `Accept-Encoding`, `Accept-Language`). Бисект BUG-1113 показал, что по
  отдельности они на тех пяти сайтах ничего не меняли; заводить дефект рано. Снято по plain HTTP на loopback, по HTTPS
  набор Chrome может отличаться.
- **Ловушки приборов:** `--user-agent` у Chrome обнуляет Client Hints (поэтому UA ставится через CDP-override с
  метаданными); `navigator.userAgentData` пуст на `about:blank` (бренды берутся на loopback); холодный старт Chrome
  изредка не открывает CDP-порт — три попытки.

## 2026-10-14 — COMPAT-CORPUS-4 — стенд G (капчи и антибот), плечо Chrome — Windows 10, маршрут home-direct

Прибор `scripts/captcha_stand.py` над `docs/perf/corpus-captcha.tsv` (20 страниц, 12 вендоров), сырой результат —
`docs/perf/runs/2026-10-14-captcha-chrome.json`. Капчи не решаются; вердикт на страницу — пройдено / петля /
не_отрисован / отказ / завис.

- **Chrome (Chromium из кэша Playwright):** GPR 14/20 = 0.70; балл reCAPTCHA v3 = 0.9. По вендорам: reCAPTCHA 4/4,
  Anubis 2/2, Akamai, CF managed, DDoS-Guard, Imperva, Qrator, human 1/1, hCaptcha 1/2, Turnstile 1/4, SmartCaptcha 0/1,
  DataDome 0/1 (`завис`: пустая страница). Turnstile managed/interactive и виджеты 2captcha-демо не отрисовываются
  даже в Chrome при чистом профиле и автоматизации — это эталон, а не дефект Lumen.
- **Lumen:** не снято. Живое окно не отдаёт первый кадр до конца `wait` (BUG-1629): `navigate` на example.com 47 с,
  19 из 20 страниц — «отказ» по таймауту, это дефект окна, а не результат стенда. База GPR Lumen — COMPAT-CORPUS-4-BASE.
- **Ловушки приборов:** демо-страница v3 в Chrome сама `fetch` на verify не отправляет (запрос не виден в CDP) —
  зонд один раз повторяет её вызовы `grecaptcha.execute` + verify; старт Chrome изредка не открывает CDP-цель
  (в длинном прогоне одна страница из 20), стенд делает 4 попытки, упавшая страница перезапускается отдельно (`rerun` в JSON).

## 2026-10-19 — COMPAT-CORPUS-4-BASE — стенд G, плечо Lumen и A/B 2×2 идентичности — Windows 10, маршрут home-direct

Тот же прибор и корпус (20 страниц, 12 вендоров), что в записи 2026-10-14; `target/dev-release/lumen.exe` от 2026-10-19 15:35
(после исправления BUG-1629/1630), живое окно `--maximized`, блокировщик рекламы выключен, `--observe-s 60`. Сырые результаты:
`runs/2026-10-19-captcha-lumen.json` (идентичность по умолчанию), `…-lumen-chrome_unbranded.json`, `…-lumen-chrome_lumen.json`.
Эталон Chrome — `runs/2026-10-14-captcha-chrome.json` (не перемерялся).

| Плечо | пройдено | GPR | не_отрисован | отказ | завис |
|---|---|---|---|---|---|
| Chrome 148 (2026-10-14) | 14 | 0.70 | 5 | 0 | 1 |
| Lumen, идентичность по умолчанию | 7 | **0.35** | 7 | 1 | 5 |
| Lumen, `LUMEN_IDENTITY=chrome_unbranded` | 7 | 0.35 | 7 | 0 | 6 |
| Lumen, `LUMEN_IDENTITY=chrome_lumen` | 6 | 0.30 | 7 | 1 | 6 |

**Балл reCAPTCHA v3 у Lumen: нет** (у Chrome 0.9) — `grecaptcha.execute` не возвращает токен, BUG-1647.

**Сравнение с прошлым прогоном:** первый прогон плеча Lumen; база для `--baseline` — `runs/2026-10-19-captcha-lumen.json`.
**Находки:**
- Где Lumen проходит, как Chrome: reCAPTCHA v2 ×3, hCaptcha demo, Akamai, Imperva, Qrator. Расхождения с Chrome (6): turnstile-nonint,
  cf-managed-challenge, recaptcha-v3-score, ddosguard-site, anubis-gnome-gitlab, human-zillow. Turnstile ×4: `TurnstileError 300030`,
  iframe не создаётся — BUG-1595 (OPEN). 2captcha-демо (hCaptcha/SmartCaptcha) не рисуются и в Chrome — эталон.
- A/B 2×2: токен `Lumen/x.y` против «безымянного» Chrome-UA — одно расхождение, `imperva-incapsula` («Pardon Our Interruption»
  при `chrome_lumen`, пройдено при `chrome_unbranded` и по умолчанию), по одному прогону на ячейку — **не вывод, а гипотеза**
  (флап Imperva на одном прогоне не исключён); `anubis-techaro` отличается шумом (см. ниже). Порог Р1 не пересматривается.
- **Прибор:** первый прогон засчитал `anubis-techaro` как «пройдено» по странице сетевой ошибки Lumen («Сайт … слишком долго не
  отвечает», текст ≥200 символов, без challenge). Добавлен `NET_ERROR_RE` в `captcha_stand.py`; запись в
  `2026-10-19-captcha-lumen.json` исправлена на «отказ» (GPR 8/20 → 7/20 = 0.35), повтор страницы даёт «завис».
- **H2 PING-разрывы (BUG-1646):** `no response to PING for 5000 ms` на anubis-techaro (9), human-zillow (11), datadome-leboncoin (5),
  akamai-nike (1); первый кадр там 29–74 с против единиц секунд на остальных. Это главная причина «завис» у anubis/zillow/leboncoin.
- `anubis-gnome-gitlab`, `ddosguard-site`, `cf-managed-challenge` — «завис» без PING-разрывов (gitlab: `rs=complete`, текст 0 симв.;
  ddosguard: `rs=interactive`) — причины не разобраны, отдельные срезы.
**Заведённые баги:** [BUG-1646](../../bugs/BUG-1646-OPEN.md) (H2 PING), [BUG-1647](../../bugs/BUG-1647-OPEN.md) (reCAPTCHA v3).
Ночной прогон с `--baseline` и блокировкой релиза — на машине владельца (COMPAT-BIZ): `python scripts/captcha_stand.py --arm lumen
--route home-direct --baseline docs/perf/runs/2026-10-19-captcha-lumen.json`.

## 2026-10-20 — COMPAT-CORPUS-2-BASE — первая база CPR корпуса C, точка abroad — Windows 10, маршрут VPN (Таллин, AS209693)

`python scripts/compat_crawl.py --corpus <главные страницы 500 сайтов корпуса C> --env LUMEN_NO_ADBLOCK=1 --no-rerun --timeout 40`, `dev-release` от `8778cac44`, Chrome for Testing 153.0.8010.12,
живое окно, вьюпорт 1920×945. Выходной IP — Таллин (EE), VPN включён: **это точка `abroad`; точка `ru` (резидентный IP РФ) не снята** — нужен отдельный запуск с отключённым VPN
(решение владельца, COMPAT-BIZ). Свод — [runs/2026-10-20-compat-crawl-corpus-c-abroad-summary.md](runs/2026-10-20-compat-crawl-corpus-c-abroad-summary.md).

| сегмент | сайтов | посчитано | **CPR** | status | walls | exceptions | pixels | a11y |
|---|---|---|---|---|---|---|---|---|
| RU (CPR-RU) | 200 | 195 | **11,8 %** (23) | 146 | 116 | 164 | 104 | 32 |
| global (CPR-Global) | 300 | 299 | **2,0 %** (6) | 109 | 90 | 268 | 60 | 12 |
| всего | 500 | 494 | **5,9 %** (29) | 255 | 206 | 432 | 164 | 44 |

Эталон не открыл 6 сайтов (imgsrc, litmarket, lenreg, cikrf, leroymerlin, konami) — вне CPR.
**Статусы Lumen (итог):** RU — OK 82, DEGRADED 55, TIMEOUT 21, HUNG 14, NET_FAIL 10, BROKEN_RENDER 8, SITE_REFUSED 6, DEAD 4; global — TIMEOUT 107, DEGRADED 41, OK 40, NET_FAIL 38,
SITE_REFUSED 33, HUNG 26, BROKEN_RENDER 12, DEAD 3.
**Разрыв по отказам (лишние стены Lumen относительно Chrome, сайтов):** `blank:near-empty` — RU 77, global 162; `net-error` — global 37, RU 0; антибот: Cloudflare-challenge 4, DataDome 4,
Akamai-reference 5 (все global), «blocked-generic» 2 (RU 1, EN 1), «unsupported-browser-ru» 1 (RU); у Chrome жёсткая стена на 6 сайтах (global). SITE_REFUSED: RU 6 из 195, global 33 из 299;
антибот-стены (Cloudflare/DataDome/Akamai/generic): RU 2, global 14.

**Отступления от протокола и что с ними делать:**
- Только главные страницы (500 URL, без внутренних): внутренние страницы удвоили бы время (≈50 с на сайт, весь прогон ≈ 7 ч).
- `--no-rerun` с сайта 8 (первые 7 перепроверены): флапы 0,6 % (3 из 494) относятся только к ним — **порог флапов ≤5 % на этом прогоне не проверен**, CPR — по одному проходу. Причина: повтор удваивает время на каждый
  проваленный сайт, а проваливается почти каждый.
- Прогон дважды перезапускался: перезапуск с `--resume` на сайте 8 (смена флагов) и на сайте 325 (`ENOSPC`, диск D 100 % — чужие `target/`); записи не потеряны (500 из 500 в `results.jsonl`).
- Доля TIMEOUT в global (107 из 300, 36 %) против RU (21 из 200, 10 %) — первая гипотеза: маршрут VPN до зарубежных CDN плюс таймаут 40 с на плечо; **не проверена**, нужна точка `ru` для сравнения и
  срез по 3–4 сайтам. CPR ниже порога 5 % для global и a11y 44 из 494 (9 %) — те же причины, что в COMPAT-CORPUS-1-BASE (расхождение дерева a11y, не разобрано).
**Заведённые баги:** нет — наблюдения. Сырьё (`results.jsonl`, 3 МБ, кадры) не коммитится (`.tmp/compat-crawl-abroad/`).
**Остаток задачи:** точка `ru` — отдельный запуск без VPN (`python scripts/compat_crawl.py --corpus … --out .tmp/compat-crawl-ru`), после него — таблица разрыва ru/abroad.

## 2026-10-20 (2) — COMPAT-CORPUS-2-BASE — база CPR корпуса C, точка ru — Windows 10, маршрут home-direct (МТС, Санкт-Петербург, AS8359)

Те же 500 главных страниц и те же флаги, что у точки abroad: `python scripts/compat_crawl.py --corpus <главные страницы 500 сайтов корпуса C> --env LUMEN_NO_ADBLOCK=1 --no-rerun --timeout 40`,
`dev-release` от `8778cac44` (тот же `lumen.exe`, не пересобирался), Chrome for Testing 153.0.8010.12, живое окно, вьюпорт по окну Lumen. Выходной IP — `81.201.16.3` (RU, резидентный).
**Системный прокси VPN (`127.0.0.1:12334`) остался включён в реестре** — он не трогался: Chrome запускался с `--no-proxy-server` (новый флаг краулера `--chrome-arg`), процесс — без `HTTP(S)_PROXY`;
Lumen ходит напрямую, пока нет `proxy =` в `data/fingerprint.toml` (его нет). Проверка до прогона: `api.ipify.org` через оба плеча даёт 11 знаков (`81.201.16.3`), у точки abroad было бы 13.
Свод — [runs/2026-10-20-compat-crawl-corpus-c-ru-summary.md](runs/2026-10-20-compat-crawl-corpus-c-ru-summary.md).

| сегмент | сайтов | посчитано | **CPR** | status | walls | exceptions | pixels | a11y |
|---|---|---|---|---|---|---|---|---|
| RU (CPR-RU) | 200 | 195 | **12,8 %** (25) | 138 | 105 | 175 | 96 | 32 |
| global (CPR-Global) | 300 | 239 | **17,2 %** (41) | 123 | 125 | 204 | 90 | 55 |
| всего | 500 | 434 | **15,2 %** (66) | 261 | 230 | 379 | 186 | 87 |

Эталон не открыл **66** сайтов (61 global + 5 RU: aniwaves, imgsrc, lenreg, cikrf, leroymerlin), 55 из 66 — `TimeoutError` Chrome при навигации (среди них youtube, instagram, linkedin, x, discord, medium,
bbc, nytimes; причина — вероятно, недоступность с РФ-IP, не проверялась): они вне CPR, поэтому **CPR-Global точки ru нельзя читать как «стало лучше»**.
**Статусы Lumen (посчитанные):** RU — OK 80, DEGRADED 54, TIMEOUT 23, HUNG 12, BROKEN_RENDER 10, NET_FAIL 8, SITE_REFUSED 6, DEAD 2; global — TIMEOUT 65, OK 41, DEGRADED 35, SITE_REFUSED 34, NET_FAIL 32,
BROKEN_RENDER 18, HUNG 14. Флапы 0 (повторов не было, `--no-rerun`).

**Разрыв ru ↔ abroad (одни и те же 500 сайтов, одна сборка Lumen):**

| | abroad (VPN, Таллин) | ru (резидентный IP РФ) |
|---|---|---|
| CPR-RU | 11,8 % (23 из 195) | 12,8 % (25 из 195); на пересечении 194 сайтов 11,9 % против 12,4 % |
| CPR-Global | 2,0 % (6 из 299) | 17,2 % (41 из 239); на пересечении 239 сайтов 2,5 % против 17,2 % |
| эталон не открыл | 6 | 66 (61 global) |
| лишние стены `blank:near-empty` RU / global | 77 / 162 | 90 / 79 |
| лишние стены `net-error` global | 37 | 31 |
| антибот-стены Lumen сверх Chrome (Cloudflare / DataDome / Akamai), global | 4 / 4 / 5 | 2 / 1 / 1 |
| TIMEOUT Lumen, RU / global (из всех 200 / 300) | 21 / 107 | 24 / 105 |

**Что показал разрыв:**
- **CPR-RU от точки не зависит** (11,8 % → 12,8 %, на общих сайтах 11,9 % → 12,4 %); 4 сайта в паритете только на ru, 3 только на abroad, 65 из 200 меняют статус Lumen между проходами
  (OK↔DEGRADED 33, HUNG→DEGRADED 12) — это разброс между двумя одиночными проходами, он порядка ±1 п. п. на CPR-RU.
- **CPR-Global 2,0 % → 17,2 % — артефакт симметричной гео-блокировки, а не улучшение Lumen:** 35 из 41 паритетных global-сайтов точки ru не были паритетными на abroad (обратного нет), и 22 из них — `SITE_REFUSED`:
  Chrome и Lumen получают одну и ту же страницу отказа (netflix 403/403, chatgpt 403/403, intuit 403/403; google 185 против 225 знаков текста). Для global брать **точку abroad**, для RU — любую.
- **Гипотеза про TIMEOUT в global не подтверждена:** маршрут VPN не причина — 105 из 300 на прямом маршруте против 107 из 300 через VPN. Причина на стороне Lumen (загрузка не доходит до готовности за 40 с у крупных
  зарубежных сайтов), нужен срез по 3–4 сайтам (например cnn-com, washingtonpost-com, samsung-com, zoom-us) — отдельной задачей, баг не заводился.
- Антибот-стены (Cloudflare/DataDome/Akamai) с RU-IP у Lumen сверх Chrome реже (4 против 13), но у эталона на этом IP 61 сайт вообще не открылся, поэтому сравнение по ним нечестное.
- `blank:near-empty` — главная лишняя стена на обеих точках (RU 77–90 сайтов, т.е. каждый второй RU-сайт), та же причина, что в COMPAT-CORPUS-1-BASE: страница отрисована почти пустой.

**Отступления от протокола и что с ними делать:** (1) только главные страницы, без внутренних; (2) `--no-rerun` на всех 500 (на abroad первые 7 перепроверялись), **порог флапов ≤5 % не проверен** — CPR по одному проходу;
(3) прогон прерывался один раз на 494 сайте (сессия закрыта, процессы Chrome/Lumen остались сиротами; убиты, `--resume`, записи не потеряны); (4) на пересечении 66 сайтов, где эталон не открылся на ru, CPR не
определён — как и 6 сайтов на abroad.
**Заведённые баги:** нет — наблюдения. Сырьё (`results.jsonl`, кадры) не коммитится (`.tmp/compat-crawl-ru/`).

## 2026-10-19 — COMPAT-CORPUS-1-BASE — первый живой прогон краулера паритета `compat_crawl.py` — Windows 10, маршрут home-direct

`python scripts/compat_crawl.py` на `corpus.txt` + `corpus-top100-foreign.txt` (112 уникальных сайтов), `target/dev-release/lumen.exe` от `main` 76e0522b2
(после BUG-1629/1630), живое окно `--maximized`, блокировщик рекламы выключен, Chrome for Testing 153.0.8010.12 с окном, вьюпорт 1920×945 по окну Lumen.
Прогон шёл с коммита `f3c79fa42` (правки №1 и №3 ниже уже внутри; №2 — после прогона). Свод — [runs/2026-10-19-compat-crawl-summary.md](runs/2026-10-19-compat-crawl-summary.md).

| | значение |
|---|---|
| проверки посчитаны | 111 из 112 (99,1 %); эталон не открыл `bilibili` |
| **CPR** | **4,5 %** (5 из 111) |
| флапы | 48 из 111 (43,2 %); все 106 неудачных сайтов перезапускались |
| прошло проверку: status / walls / exceptions / pixels / a11y | 76 / 74 / 100 / 52 / 11 из 111 |
| статусы Lumen (итог) | OK 25, NET_FAIL 22, TIMEOUT 19, SITE_REFUSED 15, DEGRADED 12, BROKEN_RENDER 11, HUNG 8 |

**Приёмка:** «5 проверок у ≥95 % сайтов» — выполнено (99,1 %). «Флапы ≤5 %» — **не выполнено** (43,2 %), и это не шум прибора, см. ниже. CPR записан.
**Сравнение с прошлым прогоном:** первый прогон краулера. Первый проход этого же дня (до правки №1, коммит `76e0522b2`, `results.jsonl` потерян при `ENOSPC`, цифры из консоли): CPR 4,5 % (5/111), флапы 49,5 %, status 68/111.
**Что нашёл прогон в самом приборе (исправлено в этой же ветке):**
1. `http_status` Lumen брался у *запрошенного* URL — это 301/302 редиректа на себя (ya, spotify, mozilla, theguardian) или 304 ревалидации кэша при повторе
   (wikipedia, lenta, duckduckgo); проверка status сравнивает класс с Chrome (200) и флапала 19 раз. Теперь статус ищется у адреса, на котором остановилась страница, 304 → 200
   (`lumen_final_status`): флапы status 19 → 10.
2. Страница сетевой ошибки Lumen («Сайт … слишком долго не отвечает», 650 знаков) считалась `OK` и проходила проверку walls, а повтор (таймаут `wait`, текста нет) её проваливал.
   Теперь такая страница — статус `NET_FAIL` и стена `net-error` (`is_net_error_page`, `NET_ERROR_RE` из `captcha_stand.py`); по записям прогона (оффлайн-оценка) флапы 48 → 39 (36,8 %). **Прогон со стеной `net-error` не перемерялся.**
3. Падения прибора на длинном прогоне: сводка падала на сайте, где эталон открылся в первом проходе, а при повторе нет (`KeyError: 'checks'`); `LiveBrowser.restart()` падала на
   `TimeoutExpired`, если `kill()` не снял `lumen.exe` (теперь добивается `taskkill /F /T`); прогон трижды обрывался на `ENOSPC` (диск D 100 % — чужие `target/`), `--resume` дочитывает.
**Находки (про Lumen):**
- **Флапы — в основном нестабильность Lumen, а не пороги.** Статус Lumen между двумя проходами меняется на 25 из 106 сайтов (OK↔NET_FAIL/TIMEOUT/HUNG/BROKEN_RENDER); после правки №2 остаётся
  39 сайтов с флапом: 18 — разный статус Lumen, 21 — тот же статус, но расходятся pixels (12), walls, exceptions, status. По пикселям 52 сайта под порогом 35 %, 2 в 35–60 %, 9 выше 60 %, у 35 кадра нет.
- **a11y — главный убийца CPR (11/111).** У 50 сайтов дерева нет (страница не загрузилась), из 61 с деревом ≥95 % совпало у 11, 80–95 % у 6, 50–80 % у 7, **<50 % у 37**. Lumen отдаёт больше узлов, чем Chrome
  (rust-lang: Chrome 57, Lumen 309; совпало 52/57), поэтому «<50 %» — вероятно расхождение имён/ролей узлов, а не нехватка узлов; **не разобрано**, нужен срез по 3–4 сайтам.
- **NET_FAIL 22 из 112: 14 — TLS-рукопожатие, `os error 10060` (≈40–45 с):** youtube, whatsapp, canva, quora, bbc, webmd, coursera, udemy, medium, soundcloud, snapchat, facebook, linkedin, discord.
  Это маршрут, не стек Lumen: сырой `ssl.wrap_socket` из Python к facebook/discord/bbc/linkedin/medium даёт `TimeoutError` 3 из 3, к example.com — `ok 0,4 с` (то же, что в записи 2026-10-06
  о режущем ClientHello прямом пути). Chrome те же сайты открывает; почему — не выяснено (гипотеза: системный прокси/QUIC/ECH). Для этих 14 сайтов CPR на home-direct не измерим — нужен прогон
  на маршруте с VPN. Остальные NET_FAIL — H2 PING ([BUG-1646](../../bugs/BUG-1646-OPEN.md), uol, cricbuzz) и прочее.
- Ответ на «обработку `http_status=None` при повторной навигации»: `None` (22 сайта) не роняет прибор, класс `none` ≠ `2xx` даёт честное расхождение status; повторный проход открывает `new_tab`,
  а не навигацию в той же вкладке; при `HUNG` окно перезапускается между проходами.
**Заведённые баги:** нет новых — наблюдения или маршрутные (TLS 10060), или уже заведённые (BUG-1646).
Сырые результаты прогона (`results.jsonl`, кадры) не коммитятся (`.tmp/compat-crawl-base2/`).

## 2026-10-15 — COMPAT-CORPUS-3 срез 2 — корпус S: 58 сценариев, калибровка по Chrome — Windows 10, маршрут home-direct

Корпус `docs/perf/scenarios/*.toml` (формат — `docs/compat-scenarios.md`): 58 сценариев в SPR-w на живых сайтах (3 стенда
самопроверки не считаются), плюс 5 плановых передач (DRM, ГОСТ, ЭП, звонки ×2). По категориям: вход 13 (вес 3), банки 5, оплата 4,
госуслуги 4 — вес 3; видео 7, почта 5, документы 3 — вес 2; поиск, новости, магазины, карты, соцсети, файл — вес 1. RU/EN —
поровну. Ночных (`mode = nightly`) 46, все проходят в Chrome; `manual` — сценарии, которые эталон не проходит или проходит нестабильно с этой точки.

- **Калибровка по Chrome for Testing 153 (`--chrome-only --headless`, 58 сценариев, 57 исполнены, один ждёт учётной записи):** прошли 47 (RU 23 / EN 24);
  из них ночных 46. Не прошли и переведены в `manual` (причина — в комментарии над сценарием): 6 с узлами Яндекс/Wildberries
  (`ERR_CONNECTION_CLOSED` с этой точки у самого Chrome), VK Видео и Хабр (документ не доходит до `complete` за 30 с, у Хабра нестабильно), Vimeo
  (ролик «processing»), PayPal и Amazon (бот-защита/капча у headless-эталона).
- **Что показала калибровка о самих оракулах:** поля входа Google/Facebook/Amazon поменяли `id` (`#identifierId`, `name=email`,
  `#ap_email_login`), Reddit держит поля в shadow DOM (ввод недоступен селектором верхнего документа), `id.tbank.ru` — 404,
  `account.mail.ru` уводит на VK ID, ЕСИА после «Войти» сбрасывает форму на QR-вход (проверяется ввод, не отправка), VK ID
  принимает только телефон — отправка не делается (SMS постороннему не уходит). Язык Chrome `--lang=ru-RU` меняет тексты
  Google/Microsoft/Reddit — регулярки двуязычные.
- **Lumen: не снято.** Живое окно до сих пор не отдаёт кадр до конца `wait` (BUG-1629, OPEN): на сборке p2-work от 14.10 все три
  стенда — `hang`. База SPR-w Lumen — после исправления BUG-1629 командой
  `python scripts/compat_scenarios.py --ref --nightly --exe <свежий lumen.exe>`; плечо `--ref` отсеет остаточные неверные оракулы.
- **Границы раннера** (docs/compat-scenarios.md §Границы): нет смены вкладок (попап OAuth, окно 3DS), нет ввода внутри чужих
  кадров, диалог выбора файла блокирует прогон. 3DS проверяется стендом с кадром ACS и `postMessage` (`stand-3ds-iframe`).

## 2026-10-20 — COMPAT-CORPUS-3 срез 3 — база SPR-w Lumen на корпусе S — Windows 10, маршрут home-direct (RU) + proxy 127.0.0.1:12334 (иностранные)

Сборка `dev-release` от `8fb3b9048` (лежит исправление BUG-1629/1630), `--maximized`, `LUMEN_NO_ADBLOCK=1`, эталон — Chrome for Testing 153 в плече `--ref`
(сценарий, не прошедший в Chrome, — `skip ref_fail`, в метрику не идёт). Два прохода: `--lang ru` напрямую и `--lang en --lumen-arg=--proxy
--lumen-arg=http://127.0.0.1:12334` (Lumen не читает системный прокси и `HTTPS_PROXY`, Chrome-эталон читает; прокси — системный адрес этой машины);
`social-telegram-channel` (`lang = ru`, иностранный узел) довыполнен через прокси. Три стенда самопроверки — `pass`.

- **База: SPR-w = 38,4 % (вес 38 из 99), 17 из 44 исполненных сценариев**; `base_reached` (≥ 40) достигнута. RU: 7/20 (вес 15 из 48, 31 %),
  EN: 10/24 (вес 23 из 51, 45 %). Пропущено 7: 5 плановых передач, 2 `ref_fail` (`login-esia`, `shop-avito` — не прошёл и Chrome). Нестабильных 3
  (`video-rutube`, `shop-avito`, `shop-amazon`). Прошли: оплата 2/3, госуслуги 2/4, вход 4/10, новости 3/3, документы 2/3, почта 1/4, поиск 1/2, банк 1/5, видео 0/4,
  магазины 0/2, соцсети 0/2, файл 0/1, карты 0/1.
- **Распознанные экраны 27 провалов:** `content` 17, `hang` 4, `cert_error` 3, `blank` 2, `wall` 1 — у каждого провала есть класс; кадр
  `.tmp/compat-scenarios/…/<id>.png` у всех, кроме `hang` (окно не отвечает). Рядом с классом раннер пишет адрес, заголовок и начало текста.
- **Банки — не дефект раннера и не «белая страница»:** Сбер, ВТБ, Альфа — `cert_error` (`untrusted certificate chain: UnknownIssuer`): цепочка НУЦ принимается
  только после согласия хоста (COMPAT-TRUST-NUC, кнопка на экране ошибки, согласие в `settings.db`), раннер согласия не даёт. Т-Банк и ФНС (`lkfl2.nalog.ru`) —
  `blank`, текста нет.
- **Зависания (`hang`, 4):** `gov-mos-ru` — «engine thread busy … running task from `app/about_to_wait.rs:287` for 34.4 s» (кадр до зависания показывает полностью
  отрисованную страницу mos.ru); `video-rutube`, `docs-pdf-arxiv`, `shop-amazon` — команда MCP не вернулась за таймаут. Родственник BUG-935 (тяжёлые страницы, путь relayout/rAF) — к P1.
- **Новый дефект:** BUG-1660 — прямой адрес `.mp4`/`.webm` открывается как текст из байтов контейнера (`video-mp4-h264`, `video-webm-vp8`).
- **Требует разбора P3 (не заведено — механизм не проверен):** у `login-microsoft` и `mail-outlook` «видимый текст» начинается с исходника скрипта
  (`!function(e,r){for(var t in r)…`); у `search-google` и `mail-mailru` сайт отдаёт ветку «включите JavaScript» (`google.com/httpservice/retry/enablejs`,
  `id.vk.ru` «JavaScript disabled»); `shop-ozon` и `mail-proton` — «нет соединения» при рабочем Chrome; у `login-ok-ru` → `login-sber-id` и `video-youtube` → адрес
  предыдущего сценария остаётся в окне после таймаута навигации; `login-google`, `mail-proton` — `Element click intercepted` (оверлей над полем).
- **Правки раннера по ходу прогона** (все — исходы, которые валили прогон traceback'ом): ошибка JS в оракуле (`querySelector(…).value` на странице без поля) — теперь промах
  оракула, а не падение; «поток движка занят» (MCP не получил ответ за 5 с) — `hang`, в том числе при снятии кадра/состояния после провала шага; класс экрана `cert_error`;
  ключи `--lang`, `--lumen-arg`. Тесты — `scripts/test_compat_scenarios.py` (41).
- **Не сделано:** шаги смены вкладки для попапа OAuth и окна 3DS (`list_tabs`/`switch_tab`/`close_tab` в MCP) — ROADMAP `COMPAT-CORPUS-3-TABS`; повторная калибровка `manual`
  с другой точки сети. Ночной прогон по расписанию не заведён: нужен прокси-аргумент в расписании (`--lang`-пара выше).
- **Диск D дважды заполнялся до 100 % во время прогона** (чужие сборки в слотах): `OSError 28` оборвал два запуска, оба продолжены `--resume`; записи, сделанные
  в момент нехватки места, удалены и выполнены заново (банки, ФНС, Telegram).

## 2026-10-15 — COMPAT-AB-IDENTITY — A/B 2×2 идентичности на корпусе из 535 сайтов — Windows 10, маршрут home-direct

Не прогон `perf_audit.py`: свод [runs/2026-10-15-ab-matrix.md](runs/2026-10-15-ab-matrix.md) (сырые записи — `2026-10-15-ab-chrome.json`, `2026-10-15-ab-lumen.json`, матрица заголовков — `2026-10-15-ab-header-matrix.md`). Корпус [corpus-ab.txt](corpus-ab.txt) (500 главных страниц corpus-c + стенд G + BUG-1113 + corpus.txt), измерено 533 (2 мёртвых), must-work 100. Плечо Chrome — Chrome for Testing 153 (JS исполняется), плечо Lumen — `--dump-source` (HTTP без JS; живое окно не отдаёт кадр, BUG-1629).

| Ячейка | OK | заблокировано |
|---|---|---|
| chrome / plain | 466 | 44 (8,3 %) |
| chrome / lumen | 467 | 41 (7,7 %) |
| lumen / plain | 472 | 50 (9,4 %) |
| lumen / lumen | 473 | 51 (9,6 %) |

**Сравнение с прошлым прогоном:** первый прогон A/B.
**Находки:**
- Токен `Lumen/x.y` блокирует 1 сайт из 533 (fiverr-com, PerimeterX 403) = 0,19 % при пороге Р1 0,5 %, must-work — 0; плечо Chrome: 0. Р1/Р2 — токен и бренд CH допустимы, решение в ADR-034.
- «Chrome пустил, Lumen заблокирован» 4,69 % (25), но стеку TLS/H2 подтверждённо приписан 1 сайт (eapteka-ru): 7 — нет российского корня в хранилище Lumen (`UnknownIssuer`: rzd, bspb, alfabank, nspk, tbank, sberbank, vtb), 2 — DNS, 7 — JS-челленджи/заглушки, которые плечо без JS не проходит, 4 антибот-отказа (t2-ru, dns-shop-ru, dellin-ru, domclick-ru) не зависят ни от TLS, ни от заголовков (отказывают и `curl_cffi impersonate=chrome`). Р4: проект TLS/H2 не открывать.
- Поправка по ходу: фразы «unsupported browser» искались в сыром исходнике и давали 16 ложных WALL_SOFT (tiktok, t-online, uber…: строки из бандлов) — теперь ищутся в видимом тексте.
- Р16 не измерим без JS-плеча Lumen.
**Заведённые баги:** нет новых (BUG-1629 уже открыт). Не решённое этим прогоном: российский корень в хранилище Lumen (7 банковских/гос сайтов) — пункт «OS-доверие» ADR-034.


---

## 2026-10-28 — LOAD-0 — база меток загрузки на сайтах пользователя — Windows 10, маршрут home-direct, без прокси

`python scripts/perf_audit.py --corpus docs/perf/corpus-user-ru.txt --mode stability --env LUMEN_NO_ADBLOCK=1`,
`target/dev-release/lumen.exe` от `main` 69b61a5cb + коммит LOAD-0 (метки). LOAD-1 в `main` ещё нет, поэтому база снята на коде
без LOAD-1 (отдельный worktree и cherry-pick не нужны). Живое окно, блокировщик рекламы выключен, `data/fingerprint.toml` без `proxy =`.
Метки — `load_timing` в `health.<pid>.log` ([metrics.md](metrics.md) §«Метки загрузки живого окна»).

| метрика | медиана, с | сайтов |
|---|---|---|
| `ready_s` | 20,34 | 26 |
| `first_content_s` | 3,10 | 1 |
| `final_applied_s` | 6,97 | 26 |
| `final_frame_s` | 6,97 | 26 |

Статусы: OK 18, BROKEN_RENDER 5, DEGRADED 3, TIMEOUT 2 (hh-ru, gazeta-ru), NET_FAIL 2 (tbank-ru, sberbank-ru — закрыты сетью).

Замечания для LOAD-1…:
- `first_content_s` есть только у `yandex-ru`: потоковая отрисовка до применения финальной страницы срабатывает почти нигде —
  это и есть поле для LOAD-1 (страница на экран без ожидания картинок). Пустая метка = «первого кадра с контентом раньше финала не было».
- `final_frame_s` ≈ `final_applied_s` (кадр сразу следует за применением): задержка страницы целиком в приходе финальной страницы, не в кадре.
- Несколько сайтов дают кратные 10 с значения (`auto-ru` 10,27; `drom-ru` 10,2; `cian-ru` 10,12; `mos-ru` 20,15; `market-yandex-ru` 30,38) — похоже на сетевые таймауты 10 с на подзапрос; перед сравнением «до/после» перепроверить повтором.
- Повторный прогон на том же коде дал `final_applied_s` pikabu 7,0/7,3, ria 15,9/16,1, vk 4,8/15,7: разброс между прогонами велик, сравнивать медианы по ≥ 2 прогонам.

| сайт | статус | ready_s | first_content_s | final_applied_s | final_frame_s |
|---|---|---|---|---|---|
| dzen-ru | OK | 2.45 | — | 1.79 | 1.79 |
| pikabu-ru | OK | 21.19 | — | 7.02 | 7.03 |
| mail-ru | BROKEN_RENDER | 20.27 | — | 4.55 | 4.55 |
| ozon-ru | OK | 0.7 | — | 0.58 | 0.58 |
| avito-ru | OK | 1.84 | — | 1.75 | 1.75 |
| habr-com | OK | 86.8 | — | 78.82 | 78.82 |
| yandex-ru | OK | 26.47 | 3.1 | 6.92 | 6.92 |
| vk-com | BROKEN_RENDER | 10.42 | — | 4.76 | 4.76 |
| rbc-ru | OK | 4.47 | — | 3.25 | 3.25 |
| ria-ru | DEGRADED | 33.42 | — | 15.88 | 15.88 |
| lenta-ru | BROKEN_RENDER | 17.2 | — | 2.92 | 2.92 |
| gosuslugi-ru | BROKEN_RENDER | 1.44 | — | 0.72 | 0.72 |
| wildberries-ru | OK | 7.25 | — | 7.09 | 7.09 |
| kinopoisk-ru | OK | 0.81 | — | 2.54 | 2.54 |
| ok-ru | OK | 45.24 | — | 6.38 | 6.38 |
| rutube-ru | DEGRADED | 52.16 | — | 25.68 | 25.68 |
| 2gis-ru | BROKEN_RENDER | 22.45 | — | 2.46 | 2.46 |
| hh-ru | TIMEOUT | — | — | — | — |
| gazeta-ru | TIMEOUT | — | — | — | — |
| kp-ru | DEGRADED | 20.27 | — | 3.56 | 3.56 |
| sports-ru | OK | 36.02 | — | 17.34 | 17.79 |
| vc-ru | OK | 25.73 | — | 15.3 | 15.3 |
| auto-ru | OK | 20.41 | — | 10.27 | 10.27 |
| drom-ru | OK | 20.33 | — | 10.2 | 10.2 |
| cian-ru | OK | 20.27 | — | 10.12 | 10.12 |
| mos-ru | OK | 30.28 | — | 20.15 | 20.15 |
| tbank-ru | NET_FAIL | — | — | — | — |
| sberbank-ru | NET_FAIL | — | — | — | — |
| rambler-ru | OK | 20.36 | — | 10.23 | 10.23 |
| market-yandex-ru | OK | 40.56 | — | 30.38 | 30.38 |
