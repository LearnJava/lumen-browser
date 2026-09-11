#!/usr/bin/env python3
"""BUG-1044: живая проба «клик мимо цели» через --mcp-live-port.

Три `click` подряд по одной странице:

    #ok     — цель под точкой, клик обязан пройти (`success`);
    #under  — цель накрыта `#over`, клик обязан ОТКАЗАТЬ, а не отчитаться
              успехом и разбудить чужой обработчик;
    #empty  — инлайн без собственного бокса, то же самое.

Печатает ответ канала и журнал страницы `window.__HITS` — кто на самом деле
получил событие. Успех = первый клик прошёл, два других отказали, и в журнале
нет ни одного чужого `target=`.

    python drive.py [url] [port]
"""
import json, os, socket, subprocess, sys, time

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
EXE = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')


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
    url = sys.argv[1] if len(sys.argv) > 1 else 'http://127.0.0.1:8763/index.html'
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 8903
    log_path = os.path.join(os.path.dirname(__file__), 'live-stderr.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen([EXE, '--mcp-live-port', str(port), '--maximized', 'about:blank'],
                            cwd=REPO, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        c.call('navigate', {'url': url})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 30000})
        time.sleep(1.0)
        for sel in ('#ok', '#under', '#empty'):
            rect = c.call('eval', {'code': (
                "(function(){var e=document.querySelector('%s');"
                "if(!e)return 'нет элемента';var r=e.getBoundingClientRect();"
                "return r.x+','+r.y+' '+r.width+'x'+r.height})()" % sel)})
            print(f'{sel} rect -> {json.dumps(rect, ensure_ascii=False)}')
            try:
                print(f'click {sel} -> ' + json.dumps(c.call('click', {'target': sel}),
                                                      ensure_ascii=False))
            except RuntimeError as e:
                print(f'click {sel} -> ОТКАЗ: {e}')
            time.sleep(0.4)
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
