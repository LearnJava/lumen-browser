# BUG-717 — `window.postMessage` doesn't clone the message and never validates `targetOrigin`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8206-8224` — `window.postMessage`)
**Найден:** P2, WPT-VENDOR-webmessaging, 2026-08-09

## Симптом

Категория `webmessaging` (`tests/wpt/webmessaging/`, 171 файл + 7
внекатегорийных зависимостей — `common/get-host-info.sub.js`,
`common/gc.js`, `common/utils.js`, `common/dispatcher/dispatcher.js`,
`html/anonymous-iframe/resources/common.js`,
`html/cross-origin-embedder-policy/credentialless/resources/common.js`,
`service-workers/service-worker/resources/test-helpers.sub.js` — довендорены
в этой же сессии, ранее давали HTTP 404) — вендорена и прогнана целиком
(`run_report.py --all --root webmessaging --recursive`, ~11:30, 136
отобранных id): **77/136 harness OK, 82/206 сабтестов**. Большая часть
провалов — TIMEOUT на `.https.` TLS-гэпе (BUG-657) и на отсутствии
отдельного browsing context у `<iframe>`/`window.open()` (BUG-480/BUG-359),
уже задокументированные корни. Оставшийся, ранее не описанный кластер —
сам `window.postMessage`:

1. **Сообщение никогда не клонируется.** `postMessage: function(message,
   targetOrigin)` (`dom.rs:8212`) делает `new MessageEvent(message)` и
   отдаёт `message` как есть — это исходная ссылка, а не структурная копия.
   Тест `webmessaging/without-ports/014.html` ("structured clone vs
   reference") явно проверяет `assert_not_equals(e.data[0], x)` — ожидает
   получить *другой* объект с тем же содержимым; получает исходный `x`.
   `MessagePort.postMessage` (`dom.rs:8996`) для сравнения делает это
   правильно: `var clone = structuredClone(message);`.

2. **`targetOrigin` не валидируется как абсолютный URL.** Спека
   (HTML LS §9.4.3, шаг "If targetOrigin is not one of `*`... run
   `USVString` → `origin` parsing, on failure throw a `SyntaxError`")
   требует `SyntaxError DOMException` при синтаксически некорректном
   значении. Код (`dom.rs:8208-8211`) делает `String(targetOrigin)` и
   просто сравнивает с `location.origin` — при несовпадении **молча
   ничего не делает** (`return`), никогда не бросает исключение. Три
   файла `with-options/*.html` проверяют это явно:
   `postMessage('', 'http://foo bar')` (пробел в host — невалидный URL)
   ожидает `assert_throws_dom(SyntaxError, ...)`, получает "did not throw".

3. **Двухаргументная `WindowPostMessageOptions`-форма не поддерживается
   вовсе.** Современная сигнатура — `postMessage(message, options)`, где
   `options.targetOrigin`/`options.transfer` (WHATWG HTML, замена старой
   `postMessage(message, targetOrigin, transfer)`). Функция принимает
   только позиционный `targetOrigin`; вызов `postMessage('', {targetOrigin:
   'http://foo bar'})` стрингифицирует весь объект в `"[object Object]"`,
   что никогда не совпадёт с `location.origin` — та же немая отмена
   доставки вместо `SyntaxError`, до `options.transfer` дело не доходит
   вовсе.

## Причина

`window.postMessage` (`dom.rs:8203-8224`) реализован как прямая передача
ссылки + грубое строковое сравнение origin, без прогона через
`structuredClone` (уже существующая, корректная функция — см. BUG-718 для
второго случая её не-использования) и без парсинга `targetOrigin` как URL
(`new URL(targetOrigin).origin`, перехват `TypeError`→`SyntaxError`).
`MessagePort.postMessage` в том же файле показывает, что клонирование
доступно и переиспользуемо — здесь оно просто не вызвано.

## Дальше

Fix scope (для P3): (1) заменить `new MessageEvent(message)` на
`new MessageEvent(structuredClone(message))`, с `try/catch` →
`DOMException DataCloneError` при неклонируемом значении; (2) добавить
валидацию `targetOrigin`: `'*'` → всегда доставлять, `'/'` → same-origin,
иначе распарсить как абсолютный URL и бросить `SyntaxError DOMException`
на ошибке парсинга (не просто "не совпало" — сейчас оба случая
неразличимы снаружи); (3) добавить двухаргументную `options`-форму
(`typeof targetOrigin === 'object'` → читать `.targetOrigin`/`.transfer`).
Затрагивает 3 файла напрямую (`resolving broken url` × 3 вариации вызова)
плюс минимум 2 файла на "structured clone vs reference"; остальные файлы
`with-ports`/`without-ports` в основном блокируются отдельно BUG-480/359
(нет browsing context у iframe/window.open) и не разблокируются этим
фиксом в одиночку.
