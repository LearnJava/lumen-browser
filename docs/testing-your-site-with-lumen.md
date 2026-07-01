# Тестирование вашего сайта в браузере Lumen — руководство для тестировщиков

Это руководство — для тех, кто хочет прогнать свои E2E/UI-тесты (клик по кнопке,
переход на другую страницу, проверка результата) через браузер Lumen, а не
пишет тесты самого движка Lumen.

Три независимых способа, от «привычного» к самому лёгкому:

| Способ | Когда выбирать | Протокол | Нужен Selenium? |
|---|---|---|---|
| [1. WebDriver BiDi](#1-webdriver-bidi-selenium-совместимый-протокол) | Уже есть Selenium/Playwright-инфраструктура, тесты на любом языке | W3C WebDriver BiDi поверх WebSocket | Частично — см. оговорку ниже |
| [2. MCP](#2-model-context-protocol-mcp) | Нужен максимально простой клиент на любом языке (включая AI-агентов) | JSON-RPC 2.0, по строке на сообщение, поверх TCP | Нет |
| [3. Наше решение: нативный Rust](#3-наше-решение-нативный-rust-без-selenium-и-без-mcp) | Тесты пишутся на Rust (юнит/интеграционные), нужна максимальная скорость | Нет протокола вообще — прямой вызов функций в одном процессе | Нет |

Все три способа управляют **одним и тем же движком** через общий типаж
`lumen_driver::BrowserSession` — набор возможностей (навигация, клик, ввод текста,
скриншот, JS-eval, поиск по CSS-селектору) один и тот же, различается только
то, как вы до него достучались.

> Все примеры в этом документе реально запускались и проверялись на живом окне
> Lumen во время подготовки документа (см. `subsystems/shell.md`/`subsystems/driver.md`
> за подробностями найденных и исправленных багов). Известные ограничения
> перечислены в конце каждого раздела — они не гипотетические, тоже проверены руками.

---

## Как получить `lumen.exe`

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"   # Git Bash, если ещё не в PATH

# Быстрая сборка для тестирования (в 2-3 раза быстрее release):
LUMEN_PROFILE=dev-release cargo build -p lumen-shell --profile dev-release

# Бинарь:
target/dev-release/lumen.exe
```

Полноценный `--release` тоже подходит (`cargo build -p lumen-shell --release`,
бинарь в `target/release/lumen.exe`) — просто дольше собирается.

---

## 1. WebDriver BiDi (Selenium-совместимый протокол)

Lumen реализует **W3C WebDriver BiDi** — тот же протокол, на котором основана
поддержка BiDi в Selenium и Playwright — напрямую поверх WebSocket:

```
ws://127.0.0.1:<port>/session
```

**Важная оговорка:** это «голый» BiDi без классического HTTP WebDriver
(`POST /session`, `/wd/hub` и т.п.) — такого эндпоинта у Lumen нет вообще, только
WebSocket. Большинство сегодняшних клиентских библиотек Selenium bootstrap-ят
сессию именно через классический HTTP-протокол, а BiDi подключают уже поверх
готовой сессии. Если ваша версия Selenium/Playwright умеет подключаться
**напрямую** по BiDi WebSocket URL без классического HTTP-хендшейка — просто
укажите ей адрес выше. Если нет — ниже дан лёгкий (и гарантированно рабочий)
клиент на «сыром» WebSocket, который говорит на том же самом протоколе, что и
Selenium/Playwright под капотом; на нём и стройте свой тестовый код.

### Запуск живого окна с BiDi

```bash
target/dev-release/lumen.exe --bidi-port 9222 --no-scrollbar about:blank
```

`--bidi-port` можно комбинировать с любым режимом запуска (открыть окно с
конкретной страницей, `--devtools-port` и т.д.) — сервер BiDi поднимается
фоновым потоком и управляет тем самым живым окном.

### Рабочий пример на Python (`websockets`)

```bash
pip install websockets
```

```python
import asyncio
import base64
import json

import websockets

PORT = 9222


class LumenBiDiClient:
    """Минимальный клиент WebDriver BiDi для Lumen — то же, что делает
    Selenium/Playwright под капотом, но без лишней обвязки."""

    def __init__(self, ws):
        self.ws = ws
        self._id = 0
        self.context = None

    @classmethod
    async def connect(cls, port: int = PORT):
        ws = await websockets.connect(f"ws://127.0.0.1:{port}/session")
        client = cls(ws)
        await client._send("session.new", {"capabilities": {}})
        tree = await client._send("browsingContext.getTree", {})
        client.context = tree["result"]["contexts"][0]["context"]
        return client

    async def _send(self, method: str, params: dict) -> dict:
        self._id += 1
        await self.ws.send(json.dumps({"id": self._id, "method": method, "params": params}))
        resp = json.loads(await self.ws.recv())
        if resp.get("type") == "error":
            raise RuntimeError(f"{method}: {resp['error']}: {resp['message']}")
        return resp

    async def navigate(self, url: str) -> None:
        await self._send("browsingContext.navigate", {"context": self.context, "url": url})

    async def screenshot(self) -> bytes:
        resp = await self._send("browsingContext.captureScreenshot", {"context": self.context})
        return base64.b64decode(resp["result"]["data"])

    async def click_at(self, x: float, y: float) -> None:
        """Клик по координатам — как в скриншоте (0,0 = левый верхний угол
        страницы; таб-бар шелла сюда не входит, движок сам это учитывает)."""
        await self._send("input.performActions", {
            "context": self.context,
            "actions": [{
                "type": "pointer", "id": "mouse",
                "actions": [
                    {"type": "pointerMove", "x": x, "y": y},
                    {"type": "pointerDown", "button": 0},
                ],
            }],
        })

    async def close(self) -> None:
        await self.ws.close()


async def main():
    client = await LumenBiDiClient.connect(9222)

    await client.navigate("file:///D:/path/to/your/site/index.html")

    # Сделать скриншот и сохранить.
    png = await client.screenshot()
    with open("before.png", "wb") as f:
        f.write(png)

    # Клик по координатам, которые вы видите на скриншоте (например, кнопка
    # «Войти» на пиксельных координатах x=30, y=92 — найдите их, открыв
    # before.png в любом редакторе изображений).
    await client.click_at(30, 92)

    # Ещё один скриншот — проверить результат клика (например, сравнить с
    # эталоном или прогнать через OCR/diff).
    png_after = await client.screenshot()
    with open("after.png", "wb") as f:
        f.write(png_after)

    await client.close()


asyncio.run(main())
```

### Что уже умеет BiDi-сервер Lumen

Полный список реализованных модулей команд:

- `session.*` — status / new / subscribe / unsubscribe / end / setDefaultUserContextLocale
- `browsingContext.*` — create / close / navigate / activate / getTree / **captureScreenshot** / handleUserPrompt / setViewport
- `script.*` — evaluate / callFunction / addPreloadScript / removePreloadScript / disown / getRealms
- `input.*` — performActions / releaseActions / setFiles
- `storage.*` — getCookies / setCookie / deleteCookies
- `network.*` — getResponseBody / setOfflineStatus / addIntercept / removeIntercept / setCacheBehavior и др.
- `browser.*` — setTimezoneOverride / getDownloads
- `emulation.*` — setUserAgentOverride

**Что реально работает против живого окна** (а не просто отвечает «ОК» в памяти):
`browsingContext.navigate`, `browsingContext.captureScreenshot`, `script.evaluate`
(см. оговорку про JS ниже), и подмножество `input.performActions` (клик мышью:
`pointerMove`+`pointerDown`; ввод текста: `key`/`keyDown`).

Всё остальное (`network.*`, cookie-события, `browsingContext.create/close`,
`handleUserPrompt`, `setViewport` и т.д.) пока только ведёт внутреннюю
bookkeeping-запись и отвечает «успех», не подключено к реальному движку —
не полагайтесь на них для реальных проверок.

### Известные ограничения BiDi

- **`script.evaluate` требует сборку с `--features quickjs`** (в `default`-фичах
  `lumen-shell` она уже включена, так что обычная сборка без флагов подходит).
  Даже с этой фичей есть отдельный найденный, ещё не починенный баг:
  **`eval` работает только для той страницы, с которой окно было запущено** —
  после первого `browsingContext.navigate` (или инструмента `navigate` в MCP)
  `script.evaluate` начинает падать с `"JS context not available"` до конца
  сессии окна. Если вам критичен JS-eval, запускайте окно сразу с нужной
  страницей (`lumen.exe --bidi-port 9222 mysite/index.html`) и не вызывайте
  `navigate` посреди сценария — либо не полагайтесь на `eval`, используйте
  клики/скриншоты/`query` (см. раздел MCP).
- `input.performActions` — реализовано только «клик мышью» и «ввод текста»,
  без `pointerUp`, без задержек/пауз, без мультитач/wheel/drag-жестов.
- `script.callFunction` всегда возвращает стаб `{type:"undefined"}` — не
  реализовано по-настоящему (используйте `script.evaluate` с готовым
  выражением вместо вызова функции с аргументами).
- Нет `browsingContext.locateNodes` — то есть **кликнуть по CSS-селектору
  напрямую через чистый BiDi нельзя**, только по пиксельным координатам.
  Если вам нужен клик по селектору — либо используйте MCP (раздел 2, там есть
  нативный `click{selector}`), либо вычислите координаты элемента сами через
  `query` из MCP (`resource://layout`/`layout.bounding_rect`), либо через
  скриншот.

---

## 2. Model Context Protocol (MCP)

MCP изначально придуман для AI-агентов, но это просто **JSON-RPC 2.0 поверх TCP,
по одному JSON-объекту в строке** — работает из любого языка без единой
внешней зависимости (даже без `websockets`), и умеет то, чего не умеет чистый
BiDi: **клик и ввод текста напрямую по CSS-селектору**.

### Запуск живого окна с MCP

```bash
target/dev-release/lumen.exe --mcp-live-port 9222 --no-scrollbar about:blank
```

Как и `--bidi-port`, комбинируется с любым режимом запуска. Особенность:
`--mcp-live-port` обслуживает **одно соединение за раз** (следующий клиент
подключится после того, как предыдущий отключится) — этого достаточно для
последовательного прогона тестов одним клиентом.

> Не путайте с `--mcp`/`--mcp-port` — это **другой**, полностью headless режим
> без окна (`InProcessSession`), предназначенный для AI-агентов вроде Claude
> Desktop через stdio. Для тестирования реального рендеринга нужен именно
> `--mcp-live-port`.

### Протокол

Один JSON-объект на строку, `\n`-разделитель, JSON-RPC 2.0:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"navigate","arguments":{"url":"file:///..."}}}
```

Ответ — тоже одна строка:

```json
{"jsonrpc":"2.0","id":1,"result":{"success":true,"url":"file:///..."}}
```

### Доступные инструменты (`tools/call`)

| Инструмент | Аргументы | Что делает |
|---|---|---|
| `navigate` | `{url}` | Перейти по URL (`file://`, `http://`, `https://`) |
| `click` | `{target}` | Клик — `target` = `{"selector": "#id"}` \| `{"node_id": N}` \| `{"point": {"x","y"}}` |
| `type` | `{target, text}` | Ввести текст в поле |
| `scroll` | `{target, delta: {x,y}}` | Прокрутить страницу |
| `wait` | `{condition, selector?, timeout_ms?}` | Дождаться условия: `document_ready` / `visible` / `stable` / `network_idle` / `js_idle` |
| `eval` | `{code}` | Выполнить JS (см. ограничение ниже) |
| `query` | `{selector}` | Найти элементы по CSS-селектору |

### Доступные ресурсы (`resources/read`)

| URI | Формат | Содержимое |
|---|---|---|
| `resource://screenshot` | PNG (base64) | Скриншот текущей страницы |
| `resource://a11y_tree` | JSON | Дерево доступности (роли ARIA, имена, состояния) |
| `resource://layout` | JSON | Box-model всех элементов (включая `bounding_rect` — полезно, если нужны координаты для BiDi-клика) |
| `resource://console` | JSON | Лог console.log/warn/error |
| `resource://network` | JSON | Лог сетевых запросов |

### Рабочий пример на Python (без внешних зависимостей)

```python
import base64
import json
import socket

PORT = 9222


class LumenMcpClient:
    """Клиент MCP для Lumen — только стандартная библиотека."""

    def __init__(self, port: int = PORT):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=10)
        self.sock.settimeout(60)  # см. примечание про --mcp-live-port ниже
        self._reader = self.sock.makefile("r", encoding="utf-8", newline="\n")
        self._id = 0

    def _call(self, method: str, params: dict) -> dict:
        self._id += 1
        req = json.dumps({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params})
        self.sock.sendall((req + "\n").encode("utf-8"))
        resp = json.loads(self._reader.readline())
        if resp.get("error") is not None:
            raise RuntimeError(f"{method}: {resp['error']}")
        return resp.get("result") or {}

    def navigate(self, url: str) -> None:
        self._call("tools/call", {"name": "navigate", "arguments": {"url": url}})

    def wait(self, condition: str = "document_ready", selector: str = None, timeout_ms: int = 10_000) -> None:
        args = {"condition": condition, "timeout_ms": timeout_ms}
        if selector:
            args["selector"] = selector
        self._call("tools/call", {"name": "wait", "arguments": args})

    def click(self, selector: str) -> None:
        self._call("tools/call", {"name": "click", "arguments": {"target": {"selector": selector}}})

    def type_text(self, selector: str, text: str) -> None:
        self._call("tools/call", {"name": "type", "arguments": {"target": {"selector": selector}, "text": text}})

    def query(self, selector: str) -> list:
        return self._call("tools/call", {"name": "query", "arguments": {"selector": selector}}).get("nodes", [])

    def screenshot(self) -> bytes:
        result = self._call("resources/read", {"uri": "resource://screenshot"})
        b64 = result["contents"][0]["data"]
        return base64.b64decode(b64)

    def close(self) -> None:
        self.sock.close()


# --- Пример сценария: логин-форма ---

client = LumenMcpClient(9222)

client.navigate("file:///D:/path/to/your/site/login.html")
client.wait("document_ready")

client.type_text("#username", "tester")
client.type_text("#password", "hunter2")
client.click("#login-button")

# Дождаться, пока после клика на странице появится ожидаемый элемент
# (например, приветственный баннер после успешного логина).
client.wait("visible", selector="#welcome-banner", timeout_ms=5000)

nodes = client.query("#welcome-banner")
assert len(nodes) == 1
assert "Добро пожаловать" in nodes[0]["text_content"]

with open("after-login.png", "wb") as f:
    f.write(client.screenshot())

client.close()
print("Тест пройден!")
```

### Известные ограничения MCP

- **Одно соединение за раз** (`--mcp-live-port`) — для параллельных тестов
  запускайте отдельные процессы `lumen.exe` на разных портах, не пытайтесь
  расшарить одно окно между потоками/тестами.
- **`eval` — то же ограничение, что и в BiDi**: работает только до первого
  `navigate`. После него `eval` возвращает ошибку `"JS context not available"`.
  Не полагайтесь на `eval` в многошаговых сценариях — `click`/`type`/`query`
  проверены и работают после любого числа переходов.
- `resource://screenshot` рендерит через **тот же движок**, что и живое окно
  (CPU-путь `render_current_page_to_png`, снимает текущий `display_list`
  напрямую) — в отличие от честного захвата экрана, ему **не нужна пауза
  «на отрисовку»**: снимок всегда актуален сразу после того, как
  `wait("document_ready")` вернул успех.
- `resource://layout`/`console`/`network` пока возвращают заглушки (пустой
  список) для live-window сессии — работают только в headless-режиме
  (`--mcp`/`--mcp-port`, `InProcessSession`).

---

## 3. Наше решение: нативный Rust (без Selenium и без MCP)

Если ваши тесты и так пишутся на Rust — самый быстрый и надёжный способ:
работать с движком **напрямую в одном процессе**, без протокола, без сокетов,
без сериализации. Это тот же типаж `BrowserSession`, что стоит за BiDi и MCP,
но без посредников.

### Подключение

`lumen-driver` пока не опубликован на crates.io (внутренний крейт монорепозитория)
— добавьте зависимость через git или путь:

```toml
[dependencies]
lumen-driver = { git = "https://your-internal-remote/lumen-browser", package = "lumen-driver" }
# или, если тесты живут внутри самого репозитория Lumen:
# lumen-driver = { path = "../lumen-browser/crates/driver" }
```

Если вам нужен `eval()` (выполнение JS), добавьте фичу `quickjs`:

```toml
lumen-driver = { git = "...", features = ["quickjs"] }
```

### Рабочий пример

```rust
use lumen_driver::{BrowserSession, Target, WinitSession};

#[test]
fn login_flow() {
    let mut session = WinitSession::new();

    // ВАЖНО: используйте голый путь до файла, БЕЗ префикса `file://` —
    // на Windows разбор `file://` URI пока не поддержан ни в WinitSession,
    // ни в InProcessSession (только явные http(s):// или голый путь).
    session
        .navigate("D:/path/to/your/site/login.html")
        .expect("не удалось загрузить страницу");

    session
        .type_text(&Target::Selector("#username".into()), "tester")
        .expect("не удалось ввести имя пользователя");
    session
        .type_text(&Target::Selector("#password".into()), "hunter2")
        .expect("не удалось ввести пароль");

    // Клик по реальной <a href>/<button> — если ссылка ведёт на другую
    // страницу, сессия реально переходит на неё (проверено).
    session
        .click(&Target::Selector("#login-button".into()))
        .expect("клик не сработал");

    // current_url() подтверждает, что переход произошёл.
    assert!(session.current_url().ends_with("welcome.html"));

    let nodes = session.query("#welcome-banner").expect("query failed");
    assert_eq!(nodes.len(), 1);
    assert!(nodes[0].text_content.contains("Добро пожаловать"));
}

// Дополнительно, со включённой фичей `quickjs`:
#[cfg(feature = "quickjs")]
#[test]
fn eval_after_login() {
    let mut session = WinitSession::new();
    session.navigate("D:/path/to/your/site/login.html").unwrap();

    let title = session.eval("document.title").expect("eval failed");
    assert_eq!(title, "\"My Site — Login\"");
}
```

Запуск: `cargo test` — как обычный Rust-тест, никакого отдельного процесса
`lumen.exe` поднимать не нужно (`WinitSession` — CPU/offscreen-рендер в том же
процессе).

### Что доступно в типаже `BrowserSession`

```rust
// Ресурсы (только чтение):
screenshot() -> Vec<u8>                       // PNG
a11y_tree() -> A11yNode                       // дерево доступности
query_a11y(&AxQuery) / query_a11y_all(...)    // поиск по роли ARIA
layout_snapshot() -> Vec<BoxModel>            // box-model всех элементов
computed_style(selector) / computed_style_snapshot(selector)
network_log() / console_log()
current_url() -> String

// Инструменты (меняют состояние):
navigate(url)
click(&Target)
type_text(&Target, text)
scroll(&Target, delta)
wait(condition, timeout_ms)
eval(js) -> String                            // требует --features quickjs
query(selector) -> Vec<NodeRef>

// Изоляция/фингерпринтинг:
fingerprint_profile() / set_fingerprint_profile(...)
user_agent() / set_user_agent(...)
set_clock(...) / set_rng_seed(...) / freeze_fingerprint(...)
```

`Target` — цель для клика/ввода/скролла:

```rust
Target::Selector("#id".into())   // CSS-селектор, первый совпавший элемент
Target::NodeId(raw_id)           // конкретный узел, полученный из query()
Target::Point { x, y }           // явные координаты (в WinitSession — no-op,
                                  // hit-test координата→узел не реализован;
                                  // используйте Selector/NodeId)
```

Больше примеров — `crates/driver/tests/cases/test_automation_commands.rs` в
самом репозитории Lumen (клик по ссылке/чекбоксу, ввод текста, eval,
проверка ошибок на неподходящих целях).

### Известные ограничения нативного Rust-варианта

- **`navigate()` не разбирает `file://` URI** (ни в `WinitSession`, ни в
  `InProcessSession`) — только `http://`/`https://` или голый путь без схемы.
  Если у вас уже есть `file://`-строка (например, скопированная из адресной
  строки браузера), обрежьте префикс сами: `url.strip_prefix("file://")`.
- `Target::Point` в `click()`/`type_text()` — заглушка (нет hit-test
  координата→DOM-узел), используйте `Target::Selector`/`Target::NodeId`.
- `eval()` — одноразовый QuickJS-рантайм на снимке текущего DOM: мутации из
  `eval` видны последующим вызовам `eval()` в рамках одного вызова, но **не
  попадают обратно** в layout/paint-состояние сессии (в отличие от живого
  окна, где рантайм персистентный). Для этого сценария (JS меняет DOM →
  проверить, что изменения отрисовались) нужен один из первых двух способов
  (BiDi/MCP), не нативный Rust.
- `type_text()` работает только на `<input type="text|password|email|tel|url|number|search">`
  и `<textarea>` — на других элементах вернёт `Err`.

---

## Что выбрать

- **Сайт уже тестируется через Selenium/Playwright, тесты на Python/Java/JS,
  и хочется минимально всё переписывать** → способ 1 (BiDi). Учитывайте
  оговорку про классический HTTP-bootstrap и используйте `LumenBiDiClient`
  из этого документа, если ваша версия клиента не умеет напрямую по BiDi.
- **Нужен самый простой, кроссплатформенный клиент без внешних зависимостей,
  с кликом по CSS-селектору «из коробки»** → способ 2 (MCP). Рекомендуется
  для большинства новых тестовых сценариев.
- **Тесты и так на Rust, важна скорость (без сетевого/IPC-оверхеда), не нужен
  живой JS после первого перехода** → способ 3 (наш нативный вариант).

Во всех трёх случаях: если тест не проходит, сначала проверьте `wait()`
(дождались ли вы нужного условия) и напечатайте/сохраните скриншот — это
обычно быстрее всего показывает, что реально произошло на странице.
