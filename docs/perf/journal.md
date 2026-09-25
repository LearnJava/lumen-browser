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

**Итог.** Заведено 28 багов, [BUG-1119](../../bugs/BUG-1119-OPEN.md)…[BUG-1146](../../bugs/BUG-1146-OPEN.md).
В 8 открытых дописаны сайты: BUG-892, 493, 568, 648, 863, 480, 970, 1114. Все отданы P6, очередь —
`STATUS-P6.md`, по числу сломанных сайтов.

| Причина | Сайты |
|---|---|
| нет `document.scripts`/`links` ([BUG-892](../../bugs/BUG-892-FIXED.md)) | imdb, espn, amazon (челлендж AWS WAF), discord |
| `document.referrer` — `undefined` ([BUG-1121](../../bugs/BUG-1121-FIXED.md)) | imgur, fandom, yahoo, yahoo-jp |
| `defer` исполняется в порядке документа ([BUG-1120](../../bugs/BUG-1120-FIXED.md)) | khanacademy, coursera |
| члены DOM не на прототипах интерфейсов ([BUG-1122](../../bugs/BUG-1122-OPEN.md)), `EventTarget` вне цепочки ([BUG-1123](../../bugs/BUG-1123-OPEN.md)), нет `CDATASection` (BUG-863) | youtube |
| `document.cookie` не сохраняется ([BUG-1119](../../bugs/BUG-1119-OPEN.md)) | msft-login |
| CSP nonce не пускает внешний скрипт ([BUG-1124](../../bugs/BUG-1124-OPEN.md)) | dropbox, gemini (гипотеза) |
| `<style>.sheet === null` сразу после вставки ([BUG-493](../../bugs/BUG-493-OPEN.md)) | twitch, quora, bbc |
| `document.write` не исполняет `<script>` ([BUG-568](../../bugs/BUG-568-OPEN.md)) | tumblr |
| `ShadowRoot` без `insertBefore` ([BUG-1130](../../bugs/BUG-1130-OPEN.md)) | archive |
| `classList` не итерируем ([BUG-1125](../../bugs/BUG-1125-OPEN.md)) | wordpress, mozilla |
| `blob:` URL не загружается ([BUG-1126](../../bugs/BUG-1126-OPEN.md)) | zoom, bing |
| `url()` во внешнем CSS от базы документа ([BUG-1127](../../bugs/BUG-1127-OPEN.md)) | apple, tumblr |
| `load` динамического скрипта после всей очереди ([BUG-1128](../../bugs/BUG-1128-OPEN.md)) | aliexpress (SystemJS) |
| `load` окна не ждёт вставленный скрипт ([BUG-1129](../../bugs/BUG-1129-OPEN.md)) | wordpress |
| прочие одиночные: `IntersectionObserverEntry`, `innerHTML` у `<script>`, `atob`, SVG с комментарием, `import.meta.resolve`, `getAttributeNames`, `History`, `HTMLDocument`, `postMessage` target, `srcset` с запятой, порядок XHR `progress`, `javaEnabled`, `BarProp`, `innerText` (BUG-1131…1144) | duolingo, bing, airbnb, tradingview, huggingface, samsung, whatsapp, yahoo-jp, webmd, amazon, apple, weibo |
| iframe `contentWindow`/`contentDocument` (BUG-480, BUG-970) | samsung, w3schools |
| `PerformanceObserver` buffered синхронно (BUG-648) | cnbc |
| 4xx/5xx заменяется страницей ошибки, `fetch` реджектит (BUG-1114) | reddit (403 и в Chrome), duolingo, fandom |

Служебные находки: [BUG-1145](../../bugs/BUG-1145-OPEN.md) (MCP `eval` отдаёт таймаут движкового
потока как «JS context not available»; мешал снять DOM на cnbc, gemini, udemy, imgur, github) и
[BUG-1146](../../bugs/BUG-1146-OPEN.md) (блокировщик игнорирует `$domain=`, виден только при
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
