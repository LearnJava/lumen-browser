#!/usr/bin/env python3
"""Перепись BUG-405 срез 48 (п.85): предсказывает ли дешёвый сигнал
`touched.is_empty() && interactive/viewport/forced-colors стабильны`
(всё уже читается `relayout_chrome_host` ДО хэширования `dl`), что байты
самого `chrome_dl` не изменились с прошлого прохода.

Срезы 43-47 объяснили цену БЕЗУСЛОВНОГО фолда overlay-команд (~130 шт.,
98.5% из них — хром браузера, срез 46) на КАЖДОМ redraw-кадре. Хром — не
единственный источник overlay (~25 мест сборки `overlay_buf` в
`redraw_requested.rs`), но самый большой (срез 46) и, в отличие от
find-bar/панелей/меню (обычно `Vec::new()`, пустой append безопасен по
построению), единственный, чьё содержимое перестраивается по ДРУГОМУ
триггеру (`relayout_chrome_host`, вызывается точечно из ~15 мест —
`grep -rn "relayout_chrome_host()"`), а не на каждом кадре — так что для
него "пусто ли предсказание" нельзя проверить построением, только
измерением.

Диагностика (`crates/shell/src/chrome_ui.rs`, под `LUMEN_FRAME_LOG=2`,
БЕЗ правки поведения) печатает на каждый вызов `relayout_chrome_host`:

    [frame] chrome-dl-repeat predict=<bool> actual=<bool>

predict — `touched.is_empty() && interactive_stable && viewport_stable
           && forced_colors_stable`, все четыре читаются ДО перезаписи
           `chrome_prev_*` этим же проходом;
actual  — тотальный хэш (`lumen_paint::hash_display_list`, тот же, что уже
           использует overlay-кэш) свежепостроенного `dl` совпал с хэшем
           прошлого прохода.

Опасное направление — predict=true, actual=false (ложно-оптимистичный
сигнал: пропуск фолда показал бы устаревший хром). Безопасное — predict=
false при actual=true (сигнал просто консервативен, цена не сэкономлена,
но пиксели верны).

Автоматизация НЕ может воспроизвести hover/`:active`-переходы (известная
готча CLAUDE.md: `InputCommand::Click`/MCP `click` не трогает хром вовсе —
`handle_click_at` не хит-тестит `chrome_hit_test`, только страницу; реальный
`CursorMoved` — единственный путь к `chrome_hovered_nid` — недостижим без
живого окна и настоящей мыши). Здесь сессия смешивает скролл (НЕ вызывает
`relayout_chrome_host` вовсе) с `new_tab`/`navigate` (гарантированно меняют
`touched.content`) — так что выборка предсказуемо смещена к predict=false;
итог честно называет этот разрыв, а не скрывает его.

    python scripts/chrome_dl_repeat_census.py --backend vulkan --rounds 12
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from scroll_perf import Client, free_port  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

REPEAT_RE = re.compile(r'chrome-dl-repeat predict=(true|false) actual=(true|false)')


def run_session(exe: str, page: str, backend: str, rounds: int, seed: int) -> str:
    env = dict(os.environ)
    env['LUMEN_FRAME_LOG'] = '2'
    if backend:
        env['WGPU_BACKEND'] = backend

    url = page
    if not url.startswith(('http://', 'https://', 'file://', 'about:')):
        url = 'file:///' + os.path.abspath(os.path.join(REPO, page)).replace('\\', '/')

    port = free_port()
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, f'chrome_dl_repeat_census_{backend or "auto"}.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen(
        [exe, '--maximized', '--mcp-live-port', str(port), 'about:blank'],
        cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        c.call('navigate', {'url': url})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        time.sleep(1.0)

        for i in range(rounds):
            # Блок скролла — НЕ вызывает relayout_chrome_host вовсе
            # (нет строк chrome-dl-repeat), но нужен для реализма кадровой
            # частоты вокруг событий хрома.
            for _ in range(6):
                c.call('scroll', {'target': 'body', 'delta': {'x': 0, 'y': 120.0}})
                time.sleep(0.05)
            # Событие хрома: новая вкладка меняет список вкладок —
            # touched.content гарантированно не пуст.
            c.call('new_tab', {'url': 'about:blank'})
            time.sleep(0.15)
            # Возврат на прежнюю страницу тем же способом — тоже меняет
            # touched.content (омнибокс/заголовок вкладки).
            c.call('navigate', {'url': url})
            c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
            time.sleep(0.15)
        time.sleep(0.4)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()
    return log_path


def parse(log_path: str) -> list[tuple[bool, bool]]:
    events: list[tuple[bool, bool]] = []
    with open(log_path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            m = REPEAT_RE.search(line)
            if m:
                events.append((m.group(1) == 'true', m.group(2) == 'true'))
    return events


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='graphic_tests/1000000-final.html')
    ap.add_argument('--rounds', type=int, default=12)
    ap.add_argument('--seed', type=int, default=1)
    args = ap.parse_args()

    exe = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')
    if not os.path.exists(exe):
        print(f'нет {exe} — cargo build -p lumen-shell --profile dev-release', file=sys.stderr)
        return 1

    print(f'стенд: {args.page}  бэкенд: {args.backend}  раундов: {args.rounds}')
    log_path = run_session(exe, args.page, args.backend, args.rounds, args.seed)
    events = parse(log_path)
    if not events:
        print(f'строк chrome-dl-repeat в логе нет — смотрите {log_path}', file=sys.stderr)
        return 1

    total = len(events)
    tt = sum(1 for p, a in events if p and a)
    tf = sum(1 for p, a in events if p and not a)   # опасное направление
    ft = sum(1 for p, a in events if not p and a)   # безопасное, но упущенная экономия
    ff = sum(1 for p, a in events if not p and not a)

    print(f'\nвсего вызовов relayout_chrome_host с диагностикой: {total}  (лог: {log_path})')
    print(f'  predict=true  actual=true   {tt:4d}  ({100.0*tt/total:5.1f}%)  — корректный пропуск')
    print(f'  predict=true  actual=false  {tf:4d}  ({100.0*tf/total:5.1f}%)  — ОПАСНО: ложный skip')
    print(f'  predict=false actual=true   {ft:4d}  ({100.0*ft/total:5.1f}%)  — консервативно, экономия упущена')
    print(f'  predict=false actual=false  {ff:4d}  ({100.0*ff/total:5.1f}%)  — корректно не пропущено')

    if tf:
        print(f'\nОПАСНОЕ направление НЕ пусто ({tf}) — гипотеза predict⇒actual НЕ подтверждена как есть.')
    else:
        print('\nОпасное направление (predict=true, actual=false) не встретилось ни разу в этой выборке.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
