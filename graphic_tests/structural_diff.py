#!/usr/bin/env python3
"""DEVX-13: структурный дифф текстовых дампов layout-дерева / display list.

Парсит текстовый вывод `lumen --dump-layout` и `lumen --dump-display-list`
(сериализаторы `lumen_layout::serialize_layout_tree` /
`lumen_paint::serialize_display_list`) в дерево и сравнивает его по
структуре, а не построчно. Вывод называет узел (путь по дереву: тип бокса
или команды + индекс среди соседей на каждом уровне) и изменившееся
свойство — вместо «изменилась строка №417», как было бы у `difflib`.

Оба формата дампа — на деле дерево:
  - layout-дамп уже вложен через отступ (2 пробела на уровень, включая
    `seg[i]`/`line[i]:`/`frag[i]` под `InlineRun`);
  - display list плоский, но вложенность (клип/трансформ/opacity/
    scroll-layer = stacking-контекст) кодируется парными
    `Push*`/`Pop*`-командами — тот же случай, другой синтаксис вложенности.

Оба разбираются одним и тем же построчным токенизатором (`_tokenize`),
который умеет не резать по пробелу внутри `"..."` (текст/alt/src) и внутри
`(...)`/`[...]` (rect, цвета, матрицы, списки шрифтов) — атрибуты вида
`key=value` остаются собственными токенами независимо от формата.

Свойства называются по-разному в двух форматах: layout-дамп пишет их как
`key=value` (`p=`, `bg=`, `w=`, ...) — токенизатор сохраняет реальное имя.
Большинство paint-команд (`FillRect (rect) color`) позиционные, без `key=`
вообще — такие аргументы получают синтетическое имя `#1`, `#2`, ... по
порядку. Держать в Python таблицу «аргумент №2 у FillRect — это color»
для ~28 вариантов `DisplayCommand` означало бы дублировать формат
сериализатора (`display_list.rs::serialize_display_list`) второй раз, с
риском молча разойтись при следующей правке формата там — вместо этого
дифф всё равно называет узел (тип команды + путь по стеку stacking-контекстов)
и старое/новое значение позиции; для layout-дерева, где и живёт основная
масса CSS-регрессий (DoD-сценарий «одно свойство одного элемента»), имя
свойства настоящее.

Используется из `dump_golden.py` вместо `difflib.unified_diff` при
несовпадении дампа с эталоном.

Запуск:
    python graphic_tests/structural_diff.py --self-test   # проверить парсер/дифф на синтетике
"""
from __future__ import annotations

import difflib
import sys
from dataclasses import dataclass, field


def _tokenize(line: str) -> list[str]:
    """Бьёт строку на токены по пробелу, не залезая внутрь `"..."`/`(...)`/`[...]`.

    `rect=(0.00, 0.00, 800.00, 0.00)` — один токен (запятые и пробелы внутри
    скобок защищены); `alt="my alt text"` — один токен (пробел внутри кавычек
    защищён); `family=["Arial","sans-serif"]` — один токен (без пробелов
    внутри самого списка, но кавычки внутри `[...]` тоже корректно закрываются).
    """
    tokens: list[str] = []
    i, n = 0, len(line)
    while i < n:
        while i < n and line[i] == ' ':
            i += 1
        if i >= n:
            break
        start = i
        paren = brack = 0
        quoted = False
        while i < n:
            c = line[i]
            if quoted:
                if c == '\\' and i + 1 < n:
                    i += 2
                    continue
                if c == '"':
                    quoted = False
                i += 1
                continue
            if c == '"':
                quoted = True
                i += 1
                continue
            if c == '(':
                paren += 1
            elif c == ')':
                paren = max(0, paren - 1)
            elif c == '[':
                brack += 1
            elif c == ']':
                brack = max(0, brack - 1)
            elif c == ' ' and paren == 0 and brack == 0:
                break
            i += 1
        tokens.append(line[start:i])
    return tokens


def _parse_line(content: str) -> tuple[str, dict[str, str]]:
    """Разбирает одну (уже без отступа) строку дампа на (kind, attrs).

    Первый токен — тип узла (`Block`, `FillRect`, `PushClipRect`, `seg[0]`,
    `line[0]:`, ...). Остальные токены вида `key=value` идут в `attrs` под
    своим ключом; токены без `=` (текст сегмента/фрагмента в кавычках)
    получают позиционный ключ `#1`, `#2`, ... — так они всё равно участвуют
    в сравнении, просто без содержательного имени свойства.
    """
    tokens = _tokenize(content)
    if not tokens:
        return '', {}
    kind = tokens[0]
    attrs: dict[str, str] = {}
    pos = 0
    for tok in tokens[1:]:
        eq = tok.find('=')
        if eq > 0 and not tok.startswith('('):
            attrs[tok[:eq]] = tok[eq + 1:]
        else:
            pos += 1
            attrs[f'#{pos}'] = tok
    return kind, attrs


@dataclass
class Node:
    """Один узел дерева дампа: layout-бокс/inline-фрагмент или paint-команда."""

    kind: str
    attrs: dict[str, str]
    children: list['Node'] = field(default_factory=list)
    line_no: int = 0


def parse_indented_tree(text: str) -> Node:
    """Строит дерево из layout-дампа по отступу (2 пробела на уровень)."""
    root = Node(kind='#root', attrs={})
    stack: list[tuple[int, Node]] = [(-1, root)]
    for line_no, raw in enumerate(text.splitlines(), start=1):
        if not raw.strip():
            continue
        depth = (len(raw) - len(raw.lstrip(' '))) // 2
        kind, attrs = _parse_line(raw.strip())
        node = Node(kind=kind, attrs=attrs, line_no=line_no)
        while len(stack) > 1 and stack[-1][0] >= depth:
            stack.pop()
        stack[-1][1].children.append(node)
        stack.append((depth, node))
    return root


def parse_display_list_tree(text: str) -> Node:
    """Строит дерево из плоского display-list дампа по парам `Push*`/`Pop*`.

    Несбалансированный `Pop*` (эталон и факт разошлись прямо на границе
    stacking-контекста) не считается ошибкой парсера: лишний `Pop` без
    открытого `Push` просто игнорируется — сам дисбаланс всплывёт в диффе
    как несовпадение количества/типа детей на уровне выше.
    """
    root = Node(kind='#root', attrs={})
    stack: list[Node] = [root]
    for line_no, raw in enumerate(text.splitlines(), start=1):
        if not raw.strip():
            continue
        kind, attrs = _parse_line(raw.strip())
        if kind.startswith('Pop'):
            if len(stack) > 1:
                stack.pop()
            continue
        node = Node(kind=kind, attrs=attrs, line_no=line_no)
        stack[-1].children.append(node)
        if kind.startswith('Push'):
            stack.append(node)
    return root


PARSERS = {
    'layout': parse_indented_tree,
    'display-list': parse_display_list_tree,
}


def _label(node: Node, idx: int) -> str:
    return f'{node.kind}[{idx}]'


def _diff_attrs(a: Node, b: Node) -> list[str]:
    if a.attrs == b.attrs:
        return []
    keys = list(dict.fromkeys([*a.attrs.keys(), *b.attrs.keys()]))
    changes = []
    for k in keys:
        av, bv = a.attrs.get(k), b.attrs.get(k)
        if av == bv:
            continue
        if av is None:
            changes.append(f'{k} добавлен: {bv}')
        elif bv is None:
            changes.append(f'{k} исчез (было {av})')
        else:
            changes.append(f'{k}: {av} -> {bv}')
    return changes


def _diff_node(a: Node, b: Node, path: str, out: list[str]) -> None:
    if a.kind != b.kind:
        out.append(f'{path}: тип изменился: {a.kind} -> {b.kind}')
        return
    changes = _diff_attrs(a, b)
    if changes:
        out.append(f'{path}: ' + '; '.join(changes))

    common = min(len(a.children), len(b.children))
    for i in range(common):
        ca, cb = a.children[i], b.children[i]
        child_path = f'{path}/{_label(ca, i)}' if ca.kind == cb.kind else f'{path}/[{i}]'
        _diff_node(ca, cb, child_path, out)
    for i in range(common, len(a.children)):
        out.append(f'{path}/{_label(a.children[i], i)}: потомок удалён')
    for i in range(common, len(b.children)):
        out.append(f'{path}/{_label(b.children[i], i)}: потомок добавлен')


def diff_trees(expected_root: Node, actual_root: Node) -> list[str]:
    """Сравнивает детей двух корней (сами `#root`-узлы синтетические, не сравниваются)."""
    out: list[str] = []
    a, b = expected_root.children, actual_root.children
    common = min(len(a), len(b))
    for i in range(common):
        label = _label(a[i], i) if a[i].kind == b[i].kind else f'[{i}]'
        _diff_node(a[i], b[i], label, out)
    for i in range(common, len(a)):
        out.append(f'{_label(a[i], i)}: потомок удалён')
    for i in range(common, len(b)):
        out.append(f'{_label(b[i], i)}: потомок добавлен')
    return out


def structural_diff(expected_text: str, actual_text: str, kind: str) -> str:
    """Структурный дифф двух дампов одного `kind` (`'layout'`/`'display-list'`).

    Вызывается только когда `expected_text != actual_text` — вызывающая
    сторона уже это проверила. Если структурный обход не находит ни одного
    различающегося узла (парсер не смог его отличить — например разошлись
    только конечные переводы строк), не молчим: отдаём обычный построчный
    дифф как страховку, с пометкой, что это fallback.
    """
    parser = PARSERS[kind]
    expected_root = parser(expected_text)
    actual_root = parser(actual_text)
    diffs = diff_trees(expected_root, actual_root)
    if not diffs:
        raw = ''.join(difflib.unified_diff(
            expected_text.splitlines(keepends=True),
            actual_text.splitlines(keepends=True),
            fromfile='expected', tofile='actual',
        ))
        return '(структурный дифф пуст, тексты всё же различаются — построчный фолбэк)\n' + raw
    return '\n'.join(diffs) + '\n'


# ---------------------------------------------------------------------------
# self-test
# ---------------------------------------------------------------------------

_LAYOUT_BEFORE = """\
Block rect=(0.00, 0.00, 800.00, 100.00)
  Block rect=(0.00, 0.00, 800.00, 50.00) p=(0.00, 0.00, 0.00, 0.00)
    InlineRun rect=(0.00, 0.00, 100.00, 20.00)
      seg[0] "hi" color=#ff0000ff
      line[0]:
        frag[0] x=0.00 "hi"
  Image rect=(0.00, 50.00, 100.00, 50.00) src="a.png" alt="a picture"
"""

_LAYOUT_AFTER_ATTR = _LAYOUT_BEFORE.replace(
    'p=(0.00, 0.00, 0.00, 0.00)', 'p=(10.00, 0.00, 0.00, 0.00)'
)

_LAYOUT_AFTER_EXTRA_CHILD = _LAYOUT_BEFORE + '  Block rect=(0.00, 100.00, 800.00, 20.00)\n'

_DL_BEFORE = """\
FillRect (0.00, 0.00, 1024.00, 720.00) #ffffffff
PushClipRect (0.00, 0.00, 100.00, 100.00)
FillRect (10.00, 10.00, 20.00, 20.00) #ff0000ff
PopClip
"""

_DL_AFTER_COLOR = _DL_BEFORE.replace(
    'FillRect (10.00, 10.00, 20.00, 20.00) #ff0000ff',
    'FillRect (10.00, 10.00, 20.00, 20.00) #00ff00ff',
)

_DL_AFTER_NEW_LAYER = (
    'FillRect (0.00, 0.00, 1024.00, 720.00) #ffffffff\n'
    'PushClipRect (0.00, 0.00, 100.00, 100.00)\n'
    'PushOpacity 0.500\n'
    'FillRect (10.00, 10.00, 20.00, 20.00) #ff0000ff\n'
    'PopOpacity\n'
    'PopClip\n'
)


def _self_test() -> int:
    failures: list[str] = []

    def check(name: str, cond: bool, detail: str = '') -> None:
        if not cond:
            failures.append(f'{name}: {detail}')

    # --- токенизатор: пробел внутри кавычек/скобок не режет токен ---
    toks = _tokenize('Image rect=(0.00, 0.00, 1.00, 1.00) src="a.png" alt="my alt text"')
    check('tokenize keeps rect intact', toks[1] == 'rect=(0.00, 0.00, 1.00, 1.00)', repr(toks))
    check('tokenize keeps quoted alt intact', toks[3] == 'alt="my alt text"', repr(toks))
    toks2 = _tokenize('DrawText (0,0,1,1) "hi" 16.00 #000000ff family=["Arial","sans-serif"]')
    check('tokenize keeps bracket list intact', toks2[-1] == 'family=["Arial","sans-serif"]', repr(toks2))

    # --- одно свойство одного элемента (layout) ---
    diff = structural_diff(_LAYOUT_BEFORE, _LAYOUT_AFTER_ATTR, 'layout')
    check('layout attr diff names Block[0]', 'Block[0]/Block[0]:' in diff, diff)
    check('layout attr diff names property p', 'p: (0.00, 0.00, 0.00, 0.00) -> (10.00, 0.00, 0.00, 0.00)' in diff, diff)
    check('layout attr diff does not touch unrelated seg/Image nodes', 'seg[0]' not in diff and 'Image[' not in diff, diff)

    # --- структурная правка (добавлен бокс) не ломает парсер и остаётся локальной ---
    diff_struct = structural_diff(_LAYOUT_BEFORE, _LAYOUT_AFTER_EXTRA_CHILD, 'layout')
    check('layout structural diff reports added child', 'потомок добавлен' in diff_struct, diff_struct)
    check('layout structural diff does not report unrelated Block[0] changes', 'Block[0]/Block[0]:' not in diff_struct, diff_struct)

    # --- display list: цвет одной команды внутри stacking-контекста ---
    diff_dl = structural_diff(_DL_BEFORE, _DL_AFTER_COLOR, 'display-list')
    check('display-list diff descends into PushClipRect', 'PushClipRect[1]/FillRect[0]:' in diff_dl, diff_dl)
    check('display-list diff names the changed color', '#ff0000ff -> #00ff00ff' in diff_dl, diff_dl)

    # --- display list: новый stacking-контекст (PushOpacity) — структурная находка ---
    diff_layer = structural_diff(_DL_BEFORE, _DL_AFTER_NEW_LAYER, 'display-list')
    check('display-list diff reports new stacking context as structural add', 'потомок добавлен' in diff_layer or 'тип изменился' in diff_layer, diff_layer)

    # --- пустой структурный дифф не тонет молча: есть текстовый фолбэк ---
    trailing_ws_diff = structural_diff(_DL_BEFORE, _DL_BEFORE + '\n', 'display-list')
    check('empty structural diff falls back to raw diff, not silence', 'фолбэк' in trailing_ws_diff, trailing_ws_diff)

    if failures:
        print('structural_diff self-test FAILED:')
        for f in failures:
            print(f'  - {f}')
        return 1
    print('structural_diff self-test passed.')
    return 0


def main(argv: list[str]) -> int:
    if '--self-test' in argv:
        return _self_test()
    print(__doc__)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
