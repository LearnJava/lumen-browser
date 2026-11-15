# BUG-1790 — Cloudflare Turnstile: челлендж-кадр работает, но не выдаёт токен (детект со стороны challenge)

**Статус:** OPEN (ДОРАБОТКА → FRAME/[BUG-480](BUG-480-OPEN.md) — срез 5 исчерпал
очередь P3: остаток требует изолята начального about:blank, а BUG-480 сам помечен
`OPEN (ДОРАБОТКА → FRAME)` и P3 его не берёт; указатель снят из `STATUS-P3.md`)
**Срез 6+:** только P1 в рамках FRAME/[BUG-480](BUG-480-OPEN.md) — начальный about:blank
должен стать документом движка со своим V8-контекстом; после этого `contentWindow.eval`
станет функцией сама и ассерт бандла кадра пройдёт.
**Заведён:** 2026-11-06 (P3, остаток [BUG-1595](BUG-1595-FIXED.md))
**Область:** js/shell — антибот-фингерпринт (`window.chrome`, `navigator.plugins`/`mimeTypes`),
`MessageEvent.isTrusted`, `Node.parentElement`, начальный кадр `contentWindow` (UX-ANTIBOT)

## Срез 5 (2026-11-15, P3)

Цель среза — снять полный стек пустого `TypeError` и назвать отсутствующий API. Сделано.

**Стек** (живая страница, realm кадра `…/turnstile/f/av0/…/new/normal`):

```
Uncaught TypeError
    at hq (<anonymous>:1:301547)
    at D (<anonymous>:1:20512)
    at Ag (<anonymous>:1:364706)
    at Ijb (<anonymous>:1:236690)
    at b8 (<anonymous>:1:336894)
    at E (<anonymous>:1:21378)
    at hL (<anonymous>:1:279196)
    at <anonymous>:1:163638
    at xn (<anonymous>:1:600444)
    at EJ (<anonymous>:1:44659)
```

Оффсеты живут в inline-`<script>` кадра (бандл меняется от запроса к запросу — имена и
константы другие, поэтому сверять надо с тем же прогоном; временный дамп текста скрипта
из `_lumen_report_exception` дал точную копию). Дизассемблирование по сохранённой копии:
`hq`/`JJ(x)` — это **собственная проверка бандла** `if (typeof x !== 'function') throw new
TypeError` (строка декодера `m9aTwInKks0=` = `function`); бросает не Lumen. Пустое
сообщение — потому что `throw new TypeError` без аргумента.

**Что за значение.** В момент броска в документе кадра ровно один `<iframe>` (без `src`;
`document.getElementsByTagName('iframe').length === 1`), его `contentWindow` — фасад
начального about:blank из среза 3, и `contentWindow.eval`/`.Function` там `undefined`.
В Chrome тот же замер: `eval`/`Function` — функции (у начального about:blank есть
настоящий realm). То есть проверка бандла падает на `contentWindow.eval` вставленного
скриптом кадра — это ровно задокументированное отклонение среза 3, а не новый
отсутствующий API.

**Эксперимент (временный, откачен):** если фасаду начального кадра отдать `eval`/
`Function` как функции (даже всегда бросающие), пустой `TypeError` исчезает и
`300010`/`failure_retry` не наступают; челлендж уходит в следующий шаг — watchdog
`300030` + `Ignored message from unexpected source for event: init/requestExtraParams/
overrunBegin`. Это повторяет замер среза 3: функции хватает, чтобы пройти ассерт, но
без настоящего realm'а дальше не идёт. Значит затыкать `eval` заглушкой нельзя —
нужен изолят начального кадра.

**Попутно измерено (не причина):** в realm кадра TT-политика создаётся
(`trustedTypes.defaultPolicy.name === "default"`), но `eval('1+1')` и
`new Function('return this')` дают `EvalError`: бандловый `createScript` возвращает
вход только для `'this'`/`'return this'`, а наш codegen-хук отдаёт ему склеенный
исходник `Function`-конструктора, и правило «exact source» его режет. Хук
`_lumen_tt_get_compliant_script_for_codegen` в этой ситуации бросает `TypeError`
и превращает его в `EvalError`; на пустой `TypeError` кадра это не влияет (там
другой путь). Отдельно: Chrome в этом кадре `eval` строкой почему-то разрешает
при том же заголовочном `require-trusted-types-for` (замер: `eval('1+1') === 2`,
`innerHTML='<b>'` не бросает) — это надо разбирать в TRUSTEDTYPES, не здесь.

**Остаток — FRAME:** начальный about:blank должен быть документом движка со своим
V8-контекстом, который подхватывает загрузка shell'а, не заменяя (это и есть
[BUG-480](BUG-480-OPEN.md)). После него `contentWindow.eval`/`.Function` станут
функциями сами, и этот ассерт пройдёт. В P3 не чинится.

Стенды среза: `.tmp/ts_capture.py` (живой прогон + GET бандла кадра),
`.tmp/ts_dumpframe.py` (попытка снять HTML кадра через BiDi — getTree не отдаёт
под-контексты), `.tmp/snap.js` + `.tmp/ts_chrome_snap.py` (снимок окружения кадра
Lumen/Chrome по именам и `typeof`), `.tmp/mkgetters.py` (геттеры-логгеры на
Chrome-only имена — кадр их не читает), `.tmp/ts_live.py` (токен).

## Срез 4 (2026-11-15, P3)

Живая страница (`peet.ws/turnstile-test/non-interactive.html`) сейчас проходит дальше среза 3:
виджет стартует, `init`/`food` доходят в обе стороны, `null.eval` не повторяется. Токена нет:
родитель получает `[Cloudflare Turnstile] Unhandled error: ` **с пустым сообщением** и debug info,
затем `300010`, затем watchdog `300030`.

**Что выяснено (временная диагностика `_lumen_report_exception`, откачена):**

* Пустой `TypeError` (`name=TypeError`, `msg=""`, стек есть, `Error.stackTraceLimit=10`) приходит
  из **обработчика `message` в realm кадра челленджа**
  (`challenges.cloudflare.com/.../turnstile/f/av0/.../new/normal`), а не из нативных вызовов:
  ни один `_lumen_*` не бросал (обёртка всех нативов молчит), `_lumen_run_classic_script` и
  `eval_and_report`/модульные пути тоже молчат. Путь вызова — `_lumen_deliver_frame_message` →
  `dispatchEvent` → слушатель страницы; сообщение, на котором он падает, — `init` **от родителя**.
* После этого Cloudflare заводит НОВЫЙ кадр `failure_retry` (`upsert_binding`: `host=35 had=false`
  → индекс `1`, тогда как исходный кадр — `host=29` → `0`) и отбрасывает его сообщения
  (`Ignored message from unexpected source for event: init/requestExtraParams/overrunBegin`):
  `api.js` проверяет `event.source === iframe.contentWindow` (`function Su`). Это **следствие**
  ретрая по `300010`, а не причина — гоняться за идентичностью фасада бессмысленно.
* Стеки в Lumen работают (кадры видны), в лог они попадают отдельными строками после
  `[JS error]`, поэтому в прошлых прогонах выглядели пустыми.

**Исправлено попутно (спек-несоответствие, найдено по пути):** у `MessageEvent` не было дефолтов
`MessageEventInit` — `event.ports` был `undefined` (HTML LS §9.3.4: пустой `sequence`), `event.source`
— `undefined` (спека: `null`), `'ports' in event`/`'source' in event` — `false`. Chrome отвечает
`[]`/`null`. Тест `message_event_has_spec_defaults_for_ports_and_source`
(`crates/js/src/v8_runtime/tests/mod.rs`).

**Следующий шаг:** снять полный стек этого `TypeError` (первый кадр — точка броска в бандле кадра) и
сопоставить с бандлом `turnstile/f/av0/...` (HTML кадра — одна строка ~610 КБ, одна inline-`<script>`;
качается обычным GET, см. `.tmp/chframe.html`), чтобы назвать отсутствующий API. Альтернатива —
диф окружения кадра против Chrome (`.tmp/ts_chrome_diag.py` — эталонная рука с console/pageerror).

Стенды: `.tmp/ts_live.py` (Lumen, живая страница), `.tmp/ts_tree.py` (контексты + проба окружения),
`.tmp/ts_chrome_diag.py` (Chrome for Testing, console/pageerror по кадрам), `.tmp/q_probe.py`
(одноразовая проба realm-свойств).

## Срез 3 (2026-11-15, P3)

Живая страница падала на `Cannot read properties of null (reading 'eval')` **в кадре
челленджа** (`doc=https://challenges.cloudflare.com/…/new/normal`): скрипт создаёт
`<iframe>` без `src` и читает `contentWindow` **синхронно в тике вставки** (временный лог
`[cw-null] src=null` в геттере; в коммит не пошёл). У Chrome это initial about:blank —
вложенный контекст по HTML LS §7.1 существует сразу, и `eval` в нём разрешён
(same-origin).

Сделано: `contentWindow`/`contentDocument` заводят начальный about:blank **на первом
чтении**, если shell ещё не зарегистрировал под-документ (`frame_bridge.rs`:
`_lumen_f_initial_frame` + `about_blank_document`; шим — `ensureInitialFrame`). Документ
несёт скелет `html/head/body` (иначе `documentElement`/`body` фасада — `null`), биндинг
`accessible: true` (about:blank наследует origin), `url = "about:blank"`; detached-элемент
по-прежнему `null` (HTML LS §4.8.5), cross-origin гейт не тронут. `upsert_binding`
сохраняет индекс, поэтому фасад переживает замену placeholder'а настоящим документом
(`contentWindow === contentWindow` до и после загрузки). Тест
`inserted_iframe_gets_initial_about_blank_window_synchronously`.

Замер: `null.eval` больше не бросается, локальный стенд по-прежнему `TOKEN`, живая
страница уходит в `300030` (watchdog «widget hung») без запроса `chl_api_ni` — челлендж
доходит до проверки `contentWindow.eval`, находит её (свойство есть) и ждёт **настоящего
realm** кадра. Подделка `eval` в изоляте читателя (и отложенный `eval` в изолят
под-документа после его загрузки) проверена и откачена: код уходит в ветку, которая молча
висит, — `300030` вместо `300010`, то есть хуже. Поэтому `eval` у начального кадра остаётся
`undefined` (отклонение задокументировано в шиме).

Остаток — **изолят начального кадра**: начальный about:blank должен быть документом
движка (его же подхватывает загрузка shell'а, не заменяя) со своим V8-контекстом. Это
FRAME-работа ([BUG-480](BUG-480-OPEN.md)), не P3-багфикс. Побочное отклонение того же
корня: всё, что страница успела записать в начальный документ до регистрации настоящего
(`document.write`, вставленные узлы), при замене документа теряется.

## Срез 2 (2026-11-15, P3)

Трассировка бриджа (`_lumen_f_post_message`/`_lumen_frame_take_messages`, временный
env-гейт `LUMEN_DEBUG_FPOST`, в коммит не пошёл) сняла подозрение срезa 1: ответ
родителя **доставляется** — `[fpost] bid=0 to="https://challenges.cloudflare.com"
matches=true`, у кадра `[ftake] … ev=["extraParams","execute"]`. Блокер был на
стороне родителя, в обработчике `requestExtraParams`.

**Дефект: `Node.parentElement` отдавал `ShadowRoot`.** `api.js` строит снимок
видимости виджета (`ui()` → `en()` → `ii()`): `for (var l = e.parentElement; l; …)`
и на каждом звене `ni()` → `ri()` → `l.getBoundingClientRect()`. Для элемента —
корня shadow-дерева — `parentElement` по DOM §4.4 должен быть `null` (родитель —
`ShadowRoot`, не элемент), а шим возвращал интернированную обёртку shadow root.
Итог: `Uncaught TypeError: e.getBoundingClientRect is not a function`, обработчик
`requestExtraParams` обрывался до `ve(...)` — кадр не получал `extraParams`,
`complete` не приходил, watchdog `300030`.

Исправлено (`crates/js/src/shim/web_api_shim_mid.js`, геттер `parentElement`):
`null`, если родитель — shadow root или `DocumentFragment` (документ уже отсекался
по BUG-557). Тест: `parent_element_is_null_inside_a_shadow_root`
(`crates/js/src/dom/tests/v8_bug1130_shadow_root_members.rs`) — без фикса падает
(`[object ShadowRoot]`/`[object Element]` вместо `null`).

Проверка: `.tmp/turnstile.html` (ключ `1x00000000000000000000AA`) — `TOKEN` за
~6 с (до фикса `ERROR 300030` через ~40 с). Прогон с tee stderr —
`.tmp/ts_run.py` (пишет `.tmp/ts_run.log`).

Остаток — **живая страница** `https://peet.ws/turnstile-test/non-interactive.html`
(реальный ключ `0x4AAAAAAABS7vwvV6VFfMcD`, `.tmp/ts_live.py`, эталон Chrome —
`.tmp/ts_live_chrome.py`: токен за ~6 с): токена нет, в кадре челленджа
`Uncaught TypeError: Cannot read properties of null (reading 'eval')`
(`[Cloudflare Turnstile] Unhandled error`, затем `300010` и `failure_retry`).
Это известный остаток [BUG-480](BUG-480-OPEN.md) (см. оговорку в
[BUG-1176](BUG-1176-FIXED.md)): челлендж пробует `iframe.contentWindow.eval(...)`,
а у вставленного скриптом `<iframe>` `contentWindow === null` даже после
`appendChild` (проба `.tmp/probe_cw.html`: `before=null after=null`). В Chrome
это cross-origin `SecurityError`, который челлендж переживает, у нас — `TypeError`,
который обрывает bootstrap. Чинится в BUG-480 (синхронный `about:blank`-WindowProxy).
Побочно на retry-виджете после этого: `Ignored message from unexpected source`
для `init`/`requestExtraParams` — следствие сорванного bootstrap, отдельно не чинить.

## Срез 1 (2026-11-15, P3)

Стенд переписан: `.tmp/turnstile.html` (локальная страница, ключ
`1x00000000000000000000AA`, `data-callback`/`data-error-callback` пишут результат
в `#st`) + `.tmp/turnstile_verify.py` (Lumen, `--bidi-port`, опрос `#st`) и
`.tmp/turnstile_chrome.py` (Chrome for Testing, плечо-эталон: `TOKEN` за 2 с).
Дифф-проба фингерпринта — `.tmp/fp1790.js`/`fp1790.py` + `fp1790b.js`.

Найдено и исправлено:

1. **`window.chrome` отсутствовал** — добавлена форма настоящего Chrome
   (`loadTimes`, `csi`, `app`, без `runtime`; ADR-034 п.1), и `navigator.plugins`/
   `mimeTypes` больше не пустые объекты, а `PluginArray` (5 PDF-плагинов) и
   `MimeTypeArray` (2 типа) с интерфейсами `Plugin`/`MimeType` — как мерил Chrome
   151 и как требует ADR-007 слой 1 (`crates/js/src/surface_api.rs`).
2. **`MessageEvent.isTrusted` был `false`** на обоих путях `postMessage`
   (`window.postMessage` и доставка из бриджа фреймов) — Chrome отдаёт `true`.
   Cloudflare Turnstile отбрасывает любое сообщение с `!e.isTrusted`
   (`Tu = e.isTrusted && …` в `api.js`), поэтому `init`/`requestExtraParams`
   до родителя доходили, но ответа кадр не получал: `Ignored message from
   unexpected source` / «widget hung» 300030 (`web_api_shim_mid_b4.js`,
   тест `post_message_and_frame_messages_are_trusted`).

После обоих фиксов родитель сообщения принимает (в логе нет
`Ignored message …`), но токена по-прежнему нет: `complete` не приходит,
через ~45 с — `ERROR 300030`. Остаток локализован до **ответа родителя в кадр**:

- `api.js`: `ve(Xe, msg, o.iframeOrigin)` → `contentWindow.postMessage(msg, "https://challenges.cloudflare.com")`;
  `Zt()` молча выходит, если `iframeOrigin` пуст, а `contentWindow` не имеет
  `postMessage`. Исключений в родителе нет (ловушка `window.onerror` пуста),
  кадр ответа не получает.
- Проверки на стенде: `Fe(...).contentWindow` из `Su` (= та же проверка, что
  делает `api.js`) отдаёт тот же фасад, что `event.source`; `targetOrigin`
  кадра — `https://challenges.cloudflare.com` (`new URL(...).origin` в Lumen
  считает верно). Дальше искать: `JSON.stringify` ответа (`wPr` содержит DOM-узлы:
  `document.body.parentElement`, `Ea(o.wrapper)`, `Oa/Aa(...)`) или проверку
  `targetOrigin`/`resolve_slot` в `_lumen_f_post_message` для cross-origin
  sandbox-кадра.
- Второй срез должен начинаться с запуска `_lumen_f_post_message` из бриджа
  фреймов **в реальном сценарии ответа** (в контексте страницы этот натив не
  виден: `window._lumen_f_post_message === undefined`, обёртка не ловит вызовы —
  он живёт в замыкании шима бриджа).


## Симптом

Страница `https://peet.ws/turnstile-test/non-interactive.html`, окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, ключ `1x00000000000000000000AA` (non-interactive, всегда pass):

- `turnstile.render()` возвращает id, challenge iframe создаётся и грузится (`GET cdn-cgi/challenge-platform/...` → 200).
- Скрипт кадра исполняется, `postMessage` доходит до родителя (`init`, `requestExtraParams`, `food`-heartbeat, `overrunBegin`/`overrunEnd`).
- `callback`/`error-callback` не дают токена: error-callback получает `"300030"` (watchdog «widget hung», ~40 с без ack), челлендж крутит overrun-ретраи без `complete`/`fail`, JS-исключений в кадре нет (console фрейма дренируется с BUG-1595 — чисто).
- В Chrome тот же стенд выдаёт токен.

## Что уже исключено

- Дефект iframe/shadow DOM — исправлен в [BUG-1595](BUG-1595-FIXED.md): iframe в closed shadow root теперь browsing context, грузится и исполняется.
- Сеть — все challenge-запросы 200, CSP/adblock не при чём (`LUMEN_NO_ADBLOCK=1`).
- Исключения JS в кадре — console фрейма дренируется в DevTools/health-log, записей нет.

## Гипотезы (по приоритету проверки)

1. ~~**`window.chrome` отсутствует**~~ — сделано в срезе 1 (форма Chrome без `runtime`).
2. ~~**`navigator.plugins`/`mimeTypes` пусты**~~ — сделано в срезе 1 (PDF-набор Chrome).
3. ~~**`MessageEvent.isTrusted === false`**~~ — сделано в срезе 1.
4. ~~**Ответ родителя (`extraParams`) не доходит до кадра**~~ — срез 2: доходит;
   обрывался обработчик `requestExtraParams` на `parentElement` → `ShadowRoot`
   (см. «Срез 2»).
5. Открыто: живой челлендж на реальном ключе — `contentWindow === null` у
   вставленного скриптом `<iframe>` (остаток [BUG-480](BUG-480-OPEN.md)).

## Как проверять

- Стенд: `python .tmp/turnstile_verify.py <path-to-lumen.exe>` (прогон ~90 с, ждёт `cf-turnstile-response`/callback).
- С трассировкой: `.tmp/ts_run.py` (`TS_PAGE=...`, пишет stderr в `.tmp/ts_run.log`).
- Живая страница: `.tmp/ts_live.py` (Lumen) против `.tmp/ts_live_chrome.py` (Chrome for Testing).
- Дифф-проба: снять тот же набор фингерпринтов в Chrome for Testing и в Lumen (`navigator.webdriver`, `window.chrome`, `plugins`, `mimeTypes`, `userAgentData`, `hardwareConcurrency`, `deviceMemory`, WebGL UNMASKED_*) и сравнить.
- Если фингерпринт выровняется, а токен всё равно не приходит — смотреть heartbeat-ack: `food`-сообщения доходят, но ответ кадра может теряться на `postMessage` targetOrigin/origin-проверках (у sandbox-кадра opaque origin).

## Связи

- Родитель: [BUG-1595](BUG-1595-FIXED.md) — iframe в shadow root.
- Блокирует остаток: [BUG-480](BUG-480-OPEN.md) — `contentWindow` вставленного скриптом iframe (`null.eval` в челлендже).
- Смежно: [BUG-754](BUG-754-FIXED.md) (`window.chrome`), UX-ANTIBOT-дорожка.

## Источник: сайты (дополнение SITES-TRIAGE-3, 2026-11-13)

medium-com (вес 3): базовый прогон — HUNG, перепрогон 2026-11-13 — run1 403 + `[Cloudflare Turnstile] Unhandled error: Cannot read properties of null (reading 'eval')`, SITE_REFUSED; Chrome проходит (229 знаков — челлендж, который Chrome решает). Тот же debug-info `IQ4G/PW4hbx…`, что в симптоме выше. vimeo-com тоже ловит Turnstile-ошибку, но Chrome там получает тот же 403 — не наш.
