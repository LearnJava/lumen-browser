# BUG-1124 — CSP: внешний `<script nonce src>` не загружается — fetch-гейт `script-src` не учитывает nonce

**Статус:** OPEN
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
