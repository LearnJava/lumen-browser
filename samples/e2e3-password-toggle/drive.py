#!/usr/bin/env python3
"""E2E-3: живая проба hit-test инлайн-переключателя «показать пароль».

Проба разделяет две гипотезы наблюдения 2026-09-09 (`type` по селектору поля
пароля отвечает `Element is not a mutable text field`):

  (1) геометрия — переключатель получает бокс шириной во всю обёртку (420px)
      и физически накрывает `<input>`;
  (2) порядок hit-test — бокс правильный, но точка в поле разрешается в
      переключатель.

Для каждого варианта печатает: rect поля, rect переключателя, пересекаются ли
они, `document.elementFromPoint` в центре поля (куда целится автоматизация) и
ответ канала на `type`. Гипотеза (1) видна по ширине бокса, (2) — по
elementFromPoint при непересекающихся боксах.

    python drive.py [url] [port]
"""
import json, os, socket, subprocess, sys, time

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
EXE = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')

VARIANTS = ('a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j')

# Геометрия обоих боксов + кто реально лежит в центре поля. Один eval на
# вариант: каждый round-trip к живому окну стоит кадра. Точка отсчёта — поле
# ввода, а где его нет (E/G) — сама обёртка: вопрос там только про бокс
# переключателя.
PROBE_JS = """(function(){
  var pw = document.querySelector('#pw-%(v)s') || document.querySelector('#wrap-%(v)s');
  var tg = document.querySelector('#toggle-%(v)s');
  if (!pw) return 'нет опорного бокса';
  function box(e){ if(!e) return null; var r=e.getBoundingClientRect();
    return {x:r.x, y:r.y, w:r.width, h:r.height}; }
  var p = box(pw), t = box(tg);
  var cx = p.x + p.w/2, cy = p.y + p.h/2;
  var at = document.elementFromPoint(cx, cy);
  var overlap = t ? !(t.x >= p.x+p.w || t.x+t.w <= p.x || t.y >= p.y+p.h || t.y+t.h <= p.y) : false;
  var cs = tg ? getComputedStyle(tg) : null;
  return JSON.stringify({
    ref: p, toggle: t, overlap: overlap,
    center: {x: cx, y: cy},
    at_center: at ? (at.id || at.nodeName) : null,
    // Отделяет «декларация не доехала до computed style» от «доехала, но
    // раскладка её не применила».
    pos: cs ? cs.position : null, right: cs ? cs.right : null,
    left: cs ? cs.left : null, top: cs ? cs.top : null
  });
})()"""


def wait_token(path, deadline):
    while time.time() < deadline:
        try:
            with open(path, encoding='utf-8', errors='replace') as fh:
                for line in fh:
                    if '[mcp] token: ' in line:
                        return line.split('[mcp] token: ', 1)[1].strip()
        except OSError:
            pass
        time.sleep(0.3)
    raise RuntimeError('токен MCP не появился')


class Client:
    def __init__(self, port, log_path):
        deadline = time.time() + 60
        token = wait_token(log_path, deadline)
        last = None
        while time.time() < deadline:
            try:
                self.sock = socket.create_connection(('127.0.0.1', port), timeout=30)
                break
            except OSError as e:
                last = e
                time.sleep(0.2)
        else:
            raise RuntimeError(f'MCP-порт {port} не поднялся: {last}')
        self.sock.settimeout(60)
        self._r = self.sock.makefile('r', encoding='utf-8', newline='\n')
        self._id = 0
        self.raw('initialize', {'token': token})

    def raw(self, method, params):
        self._id += 1
        self.sock.sendall((json.dumps({'jsonrpc': '2.0', 'id': self._id,
                                       'method': method, 'params': params}) + '\n').encode())
        line = self._r.readline()
        if not line:
            raise RuntimeError('MCP-соединение закрыто (окно упало?)')
        resp = json.loads(line)
        if resp.get('error') is not None:
            raise RuntimeError(f'{method}: {resp["error"]}')
        return resp.get('result') or {}

    def call(self, name, arguments):
        return self.raw('tools/call', {'name': name, 'arguments': arguments})


def main():
    url = sys.argv[1] if len(sys.argv) > 1 else 'http://127.0.0.1:8764/index.html'
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 8904
    log_path = os.path.join(os.path.dirname(__file__), 'live-stderr.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen([EXE, '--mcp-live-port', str(port), '--maximized', 'about:blank'],
                            cwd=REPO, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        c.call('navigate', {'url': url})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 30000})
        time.sleep(1.0)
        for v in VARIANTS:
            geo = c.call('eval', {'code': PROBE_JS % {'v': v}})
            print(f'--- вариант {v.upper()}')
            print(f'  geo  -> {json.dumps(geo, ensure_ascii=False)}')
            has_input = c.call('eval', {
                'code': "document.querySelector('#pw-%s') ? 'y' : 'n'" % v})
            if 'y' in json.dumps(has_input):
                try:
                    r = c.call('type', {'target': f'#pw-{v}', 'text': 'secret'})
                    print(f'  type -> {json.dumps(r, ensure_ascii=False)}')
                except RuntimeError as e:
                    print(f'  type -> ОТКАЗ: {e}')
                time.sleep(0.3)
                val = c.call('eval', {'code': (
                    "(function(){var e=document.querySelector('#pw-%s');"
                    "return e ? e.value : 'нет поля'})()" % v)})
                print(f'  value-> {json.dumps(val, ensure_ascii=False)}')
        hits = c.call('eval', {'code': 'JSON.stringify(window.__HITS || [])'})
        print('__HITS -> ' + json.dumps(hits, ensure_ascii=False))
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()


if __name__ == '__main__':
    sys.exit(main() or 0)
