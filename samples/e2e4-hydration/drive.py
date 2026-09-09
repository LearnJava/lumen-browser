#!/usr/bin/env python3
"""E2E-4: гоняет пробу гидрации React 18 в живом окне Lumen через --mcp-live-port.

Headless-дампы видят только синхронный скрипт (docs/engine-gaps.md), а гидрация
React 18 планируется через Scheduler, поэтому результат читается из живого окна.

    python drive.py <url> [seconds]
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

    def read(self, uri):
        return self.raw('resources/read', {'uri': uri})


def text_of(res):
    out = []
    for c in (res.get('content') or res.get('contents') or []):
        out.append(c.get('text', ''))
    return '\n'.join(out) if out else json.dumps(res)


def main():
    url = sys.argv[1]
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 20.0
    port = 8901
    log_path = os.path.join(os.path.dirname(__file__), 'live-stderr.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen([EXE, '--mcp-live-port', str(port), '--maximized', 'about:blank'],
                            cwd=REPO, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        c.call('navigate', {'url': url})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 30000})
        deadline = time.time() + seconds
        last = '[]'
        while time.time() < deadline:
            res = c.call('eval', {'code': 'JSON.stringify(window.__PROBE || [])'})
            last = text_of(res)
            if 'DONE' in last:
                break
            time.sleep(0.5)
        print('=== __PROBE ===')
        try:
            for item in json.loads(json.loads(last) if last.startswith('"') else last):
                print(' -', item)
        except Exception:
            print(last)
        print('=== console after hydrate ===')
        print(text_of(c.read('resource://console'))[:4000])

        # Фаза 2: интерактивность. React 18 вешает слушатели на корневой
        # контейнер, а не на кнопку, — нативный клик обязан всплыть до него.
        # У ступени `doc.html` кнопки нет вовсе, поэтому отказ клика здесь не
        # должен ронять прогон: измерение гидрации уже напечатано выше.
        has_btn = 'true' in c.call(
            'eval', {'code': "String(!!document.getElementById('btn'))"}).get('result', '')
        if not has_btn:
            print('=== click #btn: пропущено, на странице нет #btn ===')
            return
        print('=== click #btn ===')
        try:
            print(json.dumps(c.call('click', {'target': '#btn'}), ensure_ascii=False))
        except RuntimeError as e:
            print(f'click не выполнен: {e}')
        time.sleep(1.5)
        print("btn.textContent -> "
              + json.dumps(c.call('eval', {'code': "document.getElementById('btn').textContent"})))
        res = c.call('eval', {'code': 'JSON.stringify(window.__PROBE)'})
        try:
            for item in json.loads(json.loads(res['result'])):
                print(' -', item)
        except Exception:
            print(json.dumps(res, ensure_ascii=False))
        print('=== console after click ===')
        print(text_of(c.read('resource://console'))[:4000])

        # Фаза 3: отделить геометрию от системы событий React. MCP-клик идёт
        # через hit-test, поэтому нулевая ширина кнопки (BUG-926) уводит его в
        # родителя (BUG-1044) и ничего не говорит о делегировании React 18.
        # `dispatchEvent` минует hit-test: если после него счётчик вырос —
        # синтетические события React работают, и блокер только в геометрии.
        print('=== dispatchEvent click on #btn (минуя hit-test) ===')
        print(json.dumps(c.call('eval', {'code': (
            "document.getElementById('btn').dispatchEvent("
            "new MouseEvent('click', {bubbles: true, cancelable: true}))")}), ensure_ascii=False))
        time.sleep(1.5)
        print("btn.textContent -> "
              + json.dumps(c.call('eval', {'code': "document.getElementById('btn').textContent"})))
        res = c.call('eval', {'code': 'JSON.stringify(window.__PROBE)'})
        try:
            for item in json.loads(json.loads(res['result'])):
                print(' -', item)
        except Exception:
            print(json.dumps(res, ensure_ascii=False))
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()
        with open(log_path, encoding='utf-8', errors='replace') as fh:
            err = [l for l in fh if 'error' in l.lower() or '[JS]' in l]
        print('=== stderr (errors/JS) ===')
        print(''.join(err[-60:]))


if __name__ == '__main__':
    sys.exit(main() or 0)
