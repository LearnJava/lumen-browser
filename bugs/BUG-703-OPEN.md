# BUG-703: `tbank.ru` не завершает гидратацию React — главная страница остаётся пустой (белой), DOM застревает на SSR-скелетоне

**Статус:** OPEN — четыре движковых дефекта на пути исправлены
(`document.head`, `element.dataset`, [BUG-571](BUG-571-FIXED.md),
[BUG-721](BUG-721-FIXED.md), см. «Что уже исправлено»); рендер главной
по-прежнему не начинается, остаток описан ниже
**Компонент:** js (шим `crates/js/src/dom.rs`), shell (исполнение динамически
вставленных скриптов)
**Найден:** живая проверка `https://www.tbank.ru/` (запрос пользователя — открыть браузер, зайти в личный кабинет), 2026-08-09
**Диагностирован:** P3, 2026-08-09

## Симптом

После полной загрузки (`document.readyState === 'complete'`, все скрипты
отчитались `Загружен скрипт` без строки `script error:`) DOM остаётся на 136
узлах — только SSR-скелетон, 0 кастомных элементов, 0 shadow root'ов.
`resource://console` пуст. Скриншот: белая страница.

`document.body.innerHTML` показывает маркер React 18 Suspense:

```html
<div class="application"><div class="aA--yu9"><!--$!--><template></template><!--/$--><div></div></div></div>
```

**Уточнение по итогам разбора:** ровно тот же HTML отдаёт сервер (проверено
`curl` — `<!--$!-->` присутствует в исходной разметке). SSR отдал
errored-boundary, и всё содержимое страницы обязан отрисовать клиент; это не
«гидратация стёрла контент», а «клиентский рендер не начался» — ни одного
ключа `__reactContainer$`/`__reactFiber$` в DOM, т.е. `createRoot` не
вызывался ни разу. В headless Edge на том же URL `.application` получает 2380
React-узлов (эталонный прогон через CDP), значит страница обязана рисоваться.

## Как удалось диагностировать (тихий отказ)

Отказ невидим по обычным каналам: бутстрап Tramvai асинхронный, а
**необработанные отклонения промисов Lumen нигде не сообщает** —
`v8::Isolate::set_promise_reject_callback` не устанавливается (смежное —
[BUG-716](BUG-716-OPEN.md)). Временный колбэк, печатающий `event`, `value` и
`value.stack` в stderr, за один прогон вскрыл всю цепочку. Это первый
инструмент, который стоит подключить на симптом «страница молча не работает».
В паре с ним — временный хук `LUMEN_DIAG_PRESCRIPT=<file>` в
`v8_runtime.rs::install_dom` (eval файла сразу после шима, то есть **до**
скриптов страницы), позволяющий инструментировать живую страницу, а не только
локальный харнесс, и эталонный прогон в headless Edge через CDP (сравнение
сетевого лога и итогового DOM даёт «как должно быть» без догадок).

## Что уже исправлено (P3, 2026-08-09)

Обе находки — реальные дыры в шиме, каждая ломает произвольные сайты, а не
только этот:

1. **`document.head` отсутствовал целиком.** Тот же раскол «живой `document`
   vs `_lumen_build_detached_document`», что и [BUG-358](BUG-358-FIXED.md):
   `body` и `documentElement` были, `head` — ни в одной из двух реализаций.
   Загрузчик чанков webpack заканчивается на
   `document.head.appendChild(script)`, поэтому на **любом** бандл-сайте
   каждый ленивый чанк падал с `TypeError: Cannot read properties of
   undefined (reading 'appendChild')`, а внутри асинхронного бутстрапа это
   исчезало без следа. Добавлены нативный `_lumen_get_head`, геттер
   `document.head` и `head`/`body` у detached-документов.
2. **`element.dataset` не существовал вовсе** (единственное упоминание в
   коде — заглушка `get dataset() { return {}; }` в `svg.rs`). Следующий шаг
   того же бутстрапа, `script.dataset.mmid = ...`, падал сразу после фикса
   (1). Реализован DOMStringMap на `Proxy`: get/set/delete/`in`/`Object.keys`
   живые поверх `data-*`, camelCase↔kebab, `SyntaxError` на невалидном имени,
   стабильная идентичность (`el.dataset === el.dataset`).

Эффект: скриптов в DOM 43 → 66, оба `TypeError` ушли, страница дошла до
загрузки чанков. Рендера всё ещё нет.

3. **`fetch()` отдавал тело чужого ответа** — [BUG-721](BUG-721-FIXED.md),
   найден и исправлен в этом же разборе (P3, 2026-08-09). Любое тело до
   64 КиБ читалось из единственного глобального слота `FetchCache`, а не из
   своего: eager pull в конструкторе `ReadableStream` вычерпывал тело в
   очередь потока и освобождал персональный слот ещё до резолва промиса,
   после чего `_consumeBody` сваливался в legacy-ветку. На этой странице 20+
   URL получали один и тот же 1447-байтовый JSON-конфиг cookie-consent, в
   том числе webpack-чанк `tramvai-web-performance-rum` — он падал с
   `SyntaxError`, модули не регистрировались, и webpack бросал
   `ChunkLoadError: Loading chunk tramvai-web-performance-rum failed.
   (missing: null)` внутри `executeCommand` Tramvai. Эффект: 19
   `Uncaught SyntaxError` → 0, `ChunkLoadError` ушёл, каждый скрипт получает
   своё тело. **Рендер главной этим не чинится** — React-узлов по-прежнему
   10 (только баннер cookie-consent), `.application` не получает
   `__reactContainer$`, то есть за BUG-721 стоит ещё минимум один дефект.

## Как найдена цепочка (техника, работает и дальше)

Приложение молчит по всем обычным каналам, но **шлёт собственную телеметрию
об ошибках**. Перехват исходящих тел (`fetch`/`XMLHttpRequest.send`/
`navigator.sendBeacon` из `LUMEN_DIAG_PRESCRIPT`) выдал точный диагноз одним
прогоном: POST на `/api/front/pwaplatform/log/collect` с
`event: init-failed` и полным `ChunkLoadError`-стеком. Это дешевле и точнее,
чем бисекция бандла (сравнить с раундами 1-4 [BUG-702](BUG-702-FIXED.md)).
Дальше — сужение по слоям: обёртка `globalThis.eval` показала, какой текст
не парсится; обёртка `resp.text` — что тело не соответствует URL; обёртка
`Response._fromFetchCache` — что `_stream_handle === 0` при непустом
`_lumen_fetch_body_length()`, то есть где именно теряется персональный слот.

Оба хука временные и в дерево не влиты (P3 — только багфиксы). Восстановить
их — две правки в `crates/js/src/v8_runtime.rs`: в конце `install_dom`, после
последнего `install_*_v8`, добавить чтение `LUMEN_DIAG_PRESCRIPT` и
`self.eval(&std::fs::read_to_string(path))` (шим и все модули к этому моменту
уже стоят, скрипты страницы — ещё нет); в `v8_thread_main`, сразу после
`v8::Isolate::new`, — `isolate.set_promise_reject_callback(cb)` с
`unsafe extern "C" fn(v8::PromiseRejectMessage)`, внутри
`v8::callback_scope!(unsafe let scope, &message)` и печать
`message.get_event()` / `get_value()` / поля `stack`. Готовые пробники
(`probe703.py` + prescript'ы под каждый слой) лежали в `.tmp/`.

## Что известно об остатке (после BUG-721)

- Бандлы приложения выполняются: глобал `wsp` (webpack chunk registry
  `platform.js`) на месте, чанки `react`/`pfphomeMain`/
  `tramvai-web-performance-rum` зарегистрированы, все микроблоки
  `boxy/mm/*.client.js` и их CSS загружены.
- Приложение доходит до своих действий: POST'ы `metrics:perf`,
  `personalized-landing-metrics` (`dco-applied-info`), `certs.certInstalled`,
  `events.pageLoad` уходят.
- `__TRAMVAI_STATE__` цел и парсится (`JSON.parse` ок, 32 стора),
  `__TRAMVAI_HTML_READY__` выставлен, `__TRAMVAI_HTML_READY_RESOLVE__` вызван.
- `DOMContentLoaded`/`readystatechange`/`load`/`pageshow`/`rAF`/
  `requestIdleCallback`/`setTimeout` — все срабатывают.
- Необработанных отклонений промисов у приложения нет: остаются только
  сетевые (`eventea-beer/event`, `mddc.tbank.ru` — TLS
  [BUG-657](BUG-657-OPEN.md), `twa/ttm/…/index.js` — 404).
- Диф сетевых логов Lumen (68 URL) против headless Edge (130 URL): Lumen ни
  разу не запрашивает `api/common/v1/session`, `session_status`,
  `id.tbank.ru/*`, `cobrowsing.tbank.ru/*`, `fingerprint.t-static.ru/*` и ни
  одной картинки контента (`cdn.tbank.ru/static/pages/files/*`,
  `imgproxy.cdn-tinkoff.ru/*`) — то есть расхождение наступает до сессионного
  бутстрапа и до рендера.
- Тупики этого раунда, повторять не нужно: обёртка глобального `logger`
  ничего не ловит (модули берут логгер из DI, не из глобала); обёртки
  `Node.prototype.appendChild`/`insertBefore` не срабатывают вовсе (элементы
  шима — обычные объекты с собственными методами, не через прототип), так что
  инструментировать вставку узлов через прототипы бессмысленно.
- Мелочь на будущее: в событии `load`, которое `_lumen_script_fire` шлёт
  динамическому `<script>`, webpack читает `event.target.src` и получает
  `null` (в сообщении `ChunkLoadError` было `(missing: null)` вместо URL) —
  `event.target` шим ставит, но `.src` на обёртке элемента не отдаётся.
  Отдельно не заводилось.

## Бывший блокер — снят 2026-08-09

[BUG-571](BUG-571-FIXED.md) исправлен (P3, 2026-08-09): алгоритм «prepare the
script element» теперь живой — вставленный в `document.head` `<script src=…>`
грузится, исполняется и шлёт `load`/`error`. Описание ниже сохранено
как история диагностики; ожидаемый эффект — не менее 11 React-узлов (столько
давал полифилл), так что следующий шаг — перепроверить страницу живым
окном и диагностировать следующий дефект в цепочке.

Историческое описание: динамически вставленный `<script>` не
выполняется. `a.l` (webpack-рантайм в `platform.<hash>.js`) кладёт
`<script data-webpack src=…>` в `document.head`; элемент в DOM есть, но
**сети по нему нет** и события `load`/`error` не приходят, поэтому промис
`a.e()` висит вечно. Прямой признак на живой странице: сторожевой
`setTimeout(…, 24e4)` из `a.l` остаётся pending; чанк
`tramvai-web-performance-rum.<hash>.chunk.js` присутствует в DOM
(`document.querySelectorAll('script[data-webpack]')`) и отсутствует в сетевом
логе.

Проверка гипотезы: полифилл BUG-571 в prescript (перехват
`appendChild`/`insertBefore` на `document.head`, `fetch` + `(0, eval)`,
диспатч `load`) сдвинул страницу с **0 до 11 React-узлов** — React начал
монтировать. До эталонных 2380 всё ещё далеко, значит за BUG-571 стоит ещё
как минимум один дефект, но он не диагностируется, пока 571 открыт.

## Что не является причиной

Не выброшенное исключение из синхронного кода (консоль пуста), не сетевой
отказ загрузки скриптов (все 200 OK), не сбой парсинга, **и не общий корень с
[BUG-702](BUG-702-FIXED.md)**: `Promise` на этой странице остался нативным
(`Promise.prototype` = `constructor|then|catch|finally`,
`typeof PromiseRejectionEvent === 'function'`), core-js полифилл не
подставляет — проверено на живой странице после фикса BUG-702.

Фоновый шум, который легко принять за причину: POST-запросы аналитики
(`/api/front/*/log/collect`, `eventea-beer/event`) падают с
`network error: EOF before status line`, а `https://mddc.tbank.ru/` — с
`TLS handshake: invalid peer certificate: UnknownIssuer` (класс
[BUG-657](BUG-657-OPEN.md)). Все они обработаны приложением и на рендер не
влияют, но заслуживают отдельной проверки.

## Возможный общий класс с BUG-702

Главная страница подключает свою пару react+platform-рантайма
(`cdn.tbank.ru/s3aas/apps/pwaplatform/prod/compiled/`), страница входа —
другую (`unic/sso-newauth/`). Общего корня нет (см. выше), но общий класс
есть: обе страницы — асинхронный бутстрап того же вендора, где любой
движковый пробел превращается в тихий отказ.
