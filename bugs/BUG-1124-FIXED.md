# BUG-1124 — CSP: внешний `<script nonce src>` не загружается — fetch-гейт `script-src` не учитывает nonce

**Статус:** FIXED 2026-09-25 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** shell (`crates/shell/src/scripts.rs:385-391` — `violating_fetch_policy(policy, ScriptSrc, url)` без nonce элемента; `crates/shell/src/csp_enforce.rs:720`)

## Симптом

dropbox отдаёт CSP `script-src 'unsafe-eval' 'strict-dynamic' 'nonce-…'` и подключает
приложение только внешними `<script nonce=… src=…>`. Lumen не загружает ни одного из них
(0 строк «Загружен скрипт» из 2), `#root` пуст: 95 узлов против 1624. gemini — CSP с
`'nonce-…' 'strict-dynamic' https:`, загружено 2 скрипта, пустой кадр; DOM снять не удалось,
поэтому для gemini это гипотеза.

Гейт в `resolve_script_sources` зовёт `violating_fetch_policy(policy, ScriptSrc, url)` только по
URL; nonce проверяется лишь для инлайна (`violating_inline_policy`). GAP-CSPENF закрыт без этого
случая.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g3/server.py`:

```python
"""Repro server for g3: sends per-path CSP headers.
/csp_nonce_ext.html  — CSP header `script-src 'nonce-abc' 'strict-dynamic'`, external <script nonce=abc src=/ext.js>
/ext.js              — sets window.EXT_RAN = true
Other paths served from this dir statically.
"""
import http.server, os, sys
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8734
os.chdir(os.path.dirname(os.path.abspath(__file__)))

PAGES = {
    '/csp_nonce_ext.html': ("script-src 'nonce-abc' 'strict-dynamic'",
        b"""<!doctype html><html><body><div id=out>not run</div>
<script nonce="abc">window.INLINE_RAN = true;</script>
<script nonce="abc" src="/ext.js"></script>
<script nonce="abc">
  // strict-dynamic: script inserted by a trusted script must also run
  var s = document.createElement('script'); s.src = '/dyn.js'; document.body.appendChild(s);
</script>
</body></html>"""),
    '/csp_nonce_ext_nostrict.html': ("script-src 'nonce-abc'",
        b"""<!doctype html><html><body><div id=out>not run</div>
<script nonce="abc">window.INLINE_RAN = true;</script>
<script nonce="abc" src="/ext.js"></script>
</body></html>"""),
}
JS = {
    '/ext.js': b"window.EXT_RAN = true; document.getElementById('out').textContent = 'ext ran';",
    '/dyn.js': b"window.DYN_RAN = true;",
}

class H(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        p = self.path.split('?')[0]
        if p in PAGES:
            csp, body = PAGES[p]
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Security-Policy', csp)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers(); self.wfile.write(body); return
        if p in JS:
            body = JS[p]
            self.send_response(200)
            self.send_header('Content-Type', 'text/javascript')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers(); self.wfile.write(body); return
        if p == '/defer_slow.html':
            body = open('defer_big.html', 'rb').read()
            half = len(body) // 2
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body[:half]); self.wfile.flush()
            import time; time.sleep(1.5)
            self.wfile.write(body[half:]); return
        if p == '/csp-report':
            self.send_response(204); self.end_headers(); return
        return super().do_GET()
    def do_POST(self):
        self.send_response(204); self.end_headers()

http.server.ThreadingHTTPServer(('127.0.0.1', PORT), H).serve_forever()
```

**Результат:** Lumen: `{inl:true, ext:false}` на обеих страницах (`/csp_nonce_ext_nostrict.html`, `/csp_nonce_ext.html`). Chrome: `{inl:true, ext:true, out:'ext ran'}`.

## Что сделать

CSP3 §6.7.2.1 «Does a request match a source list» / §6.1.1.1 `script-src` pre-request
check: для запроса с nonce элемента, совпадающим с `'nonce-…'` политики, — «Matches» без проверки
URL; при `'strict-dynamic'` host-source и `'self'` игнорируются, а nonce/hash решают. Передать
nonce `<script>` в fetch-гейт. Критерий: обе страницы репро дают `ext:true`; dropbox перемерить.

## Исправление (P6, 2026-09-25)

**Корень.** Гейт внешних парсерных `<script src>` в `resolve_script_sources` звал
`violating_fetch_policy(policy, ScriptSrc, url)`. Это общий гейт fetch-директив, и он смотрит
только на URL. Список `'nonce-abc' 'strict-dynamic'` URL не покрывает ни одним
источником, поэтому каждый `<script nonce src>` блокировался ещё до запроса. Первые два
шага CSP3 §6.7.1.1 (nonce элемента, хэши `integrity`) и шаг 1.3 (`'strict-dynamic'`) выпали.

**Правка.**
- `crates/network/src/csp.rs:242` — `CspPolicy::script_element_fetch_allows(url, self_origin,
  &ScriptRequestMetadata)`: pre-request check директив скриптов по §6.7.1.1 шаг 1.
  Совпавший непустой nonce пропускает запрос при любом URL. Хэши `integrity` пропускают его,
  если в списке есть hash-источники и каждый хэш метаданных среди них; неизвестные алгоритмы
  не учитываются, пустые метаданные обход не дают. При `'strict-dynamic'` парсерный скрипт
  блокируется, остальные проходят, host-источники и `'self'` не смотрятся. Иначе решает URL.
  Цепочка `script-src` → `default-src` прежняя (`effective_sources`).
  `ScriptRequestMetadata` (`:376`) — nonce, `integrity`, `parser_inserted`.
- `crates/shell/src/csp_enforce.rs:761` — `violating_script_element_policy`: аналог
  `violating_fetch_policy` для элемента `<script>`.
- `crates/shell/src/scripts.rs:410` — гейт передаёт `nonce`/`integrity` узла и
  `parser_inserted: true` (весь этот путь вставлен парсером).

**Тесты.** `crates/network/src/csp.rs:1086`-`1150` — шесть юнит-тестов: nonce при любом URL;
nonce и `'strict-dynamic'` для парсерного скрипта (без nonce — блок, неверный nonce — блок,
вставленный скриптом — пропуск); без `'strict-dynamic'` решает URL; обход по `integrity`
требует каждый хэш; без hash-источников `integrity` не обходит; откат на `default-src`.
`crates/shell/src/tests/scripts_and_frames.rs:62` —
`resolve_script_sources_lets_a_nonced_external_script_through_csp`: `<script nonce=abc src>`
проходит гейт, а без nonce и с чужим nonce остаётся `blocked_by_csp`.

**Живая проверка** (dev-release, видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, MCP eval):

| Страница | До | После | Chrome 153 |
|---|---|---|---|
| `/csp_nonce_ext_nostrict.html` | `inl:true, ext:false` | `inl:true, ext:true, out:'ext ran'` | то же |
| `/csp_nonce_ext.html` | `inl:true, ext:false` | `inl:true, ext:true, dyn:true` | то же |
| dropbox.com | 0 из 2 «Загружен скрипт», 95 узлов | 2 из 2, **1567** узлов, 1375 под `#root` | 1630 |

Повторно на `main` с PERF-13 (одно HTTP/2-соединение на origin), три загрузки dropbox:
(1) `ready` за 123 с, 2 из 2 скриптов, `eval` → `JS context not available`
([BUG-1145](BUG-1145-OPEN.md)); (2) `ready` за 45 с, **1574** узла, 1426 под `#root`, в логе
1 строка «Загружен скрипт» — лог перезаписан следующим прогоном, причину не установил;
(3) `ready` за 133 с, 2 из 2, зонд упал на втором `eval`.

На dropbox один динамический чанк (`c_api_v2_unauthed_client-*.js`) не загрузился:
TLS-рукопожатие дважды оборвалось EOF. CSP тут ни при чём: `connect-src https://*`
его пропускает. Причина — шторм свежих рукопожатий: 147 запросов, 14 рукопожатий оборвались
(8 на `cfl.dropboxstatic.com`). Контроль: 30 параллельных `curl` к тому же URL дают 26×200,
2× `schannel: failed to receive handshake` и 2 таймаута. Это класс
[BUG-1115](BUG-1115-FIXED.md) (одно HTTP/2-соединение на origin, влит параллельно с этой
правкой). Замер снят на сборке без PERF-13.

**Найдено по ходу, заведено отдельно.** `dyn:true` на `/csp_nonce_ext.html` получен не через
`'strict-dynamic'`. Вставленный скриптом `<script src>` грузится через JS-`fetch()`, и его
судит `connect-src`, а `script-src` не смотрится вовсе. Страница с `script-src 'nonce-abc'`
без `'strict-dynamic'` исполняет вставленный скрипт, Chrome — нет. Это
[BUG-1175](BUG-1175-FIXED.md).
