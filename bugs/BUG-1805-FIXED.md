# BUG-1805 — rutube.ru: нет `navigator.serviceWorker.startMessages` (нестандартный Chrome-API контейнера)

**Статус:** FIXED 2026-11-17 (дефект закрыт работой SW-MSG-S4 P1; живая проверка и оформление — P3)
**Открыт:** 2026-11-13 (P2, SITES-TRIAGE-3 — разбор зависаний abroad)
**Закрыт:** 2026-11-17 (P3)
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — `_sw_container`, рядом `_sw_make_registration`)

## Симптом

`https://rutube.ru/` (живое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, без прокси, прогон
`compat_crawl` 2026-11-13, `.tmp/hang30-rerun/results.jsonl`): обе попытки `TIMEOUT` 41–44 с,
видимый текст 0, у Chrome 17–18 тысяч знаков. В stderr повторяется:

```
[JS error] Uncaught TypeError: navigator.serviceWorker.startMessages is not a function
script error: JS runtime error: navigator.serviceWorker.startMessages is not a function
```

Исключение убивает инициализацию плеера/страницы: headless `--trace-nav` доходит до краски
за 34 с (paint 14,9 с), но `DocumentReady` в живом окне не наступает за 40 с — страница
остаётся без текста.

## Причина

`navigator.serviceWorker.startMessages()` — нестандартный метод контейнера Chrome (включает
доставку `message`-событий от активного service worker до первого подписчика). В шиме
`_sw_container` его не было, как и в `_sw_make_registration`/`_sw_make_worker`; поиск по
`startMessages` по `crates/` был пуст. Rutube зовёт его без оговорки — падение фатально для
инициализации.

## Срез 1 (P3, 2026-11-17) — метод уже на HEAD (SW-MSG-S4), живая проверка: TypeError исчез, страница рендерит; остаточный флап вынесен в BUG-1822

- `startMessages` появился на `_sw_container` с SW-MSG-S4 (`fe55de1d5`, 2026-11-17, P1 —
  очередь сообщений клиента: события до подписки держатся, очередь открывают `onmessage`
  или явный `startMessages()`); возврат `undefined`, как у Chrome. Пункт 2 карточки
  («дозаполнить смежные») не потребовался: `getRegistrations` и `ready` уже были на месте.
  Код бага не менялся.
- Живая проверка (каноничная команда карточки, corpus-файл `rutube-ru`, dev-release на
  HEAD `de94c43ed`, 2 запуска × 2 попытки, логи `.tmp/compat-crawl/20261117-084551` и
  `20261117-085051`, не коммитятся): `startMessages` в stderr — **0** за все 4 попытки
  (было: спам в каждой попытке 2026-11-13). Попытка 1 первого запуска: статус OK, текст
  **17 112** знаков против 17 999 у Chrome (95 %), `ready_s` 41,6 — полный рендер, который
  раньше был недостижим (2/2 TIMEOUT, текст 0).
- Остаток — другой дефект, не API: 3 из 4 попыток — text=0 и `wait timeout: DocumentReady`
  на 42–45 с без единой JS-ошибки (тихий флап «полный рендер ↔ text=0»; sw.js rutube при
  этом ни разу не запросил). Вынесен в [BUG-1822](BUG-1822-OPEN.md); из-за него приёмка
  «статус не TIMEOUT» остаётся нестабильной (1/4).
