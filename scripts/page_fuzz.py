#!/usr/bin/env python3
"""TEST-2: генеративный DOM/layout-фаззер целых страниц (Domato-style).

Генерирует случайные почти-валидные HTML/CSS/JS-страницы из ограниченного,
заведомо поддерживаемого движком подмножества (теги — по `BoxKind`/UA-стилям
движка, CSS-свойства — по ✅-списку `CSS-SPECS.md`), прогоняет их headless
через `lumen.exe --dump-layout` с таймаутом и детектит три сигнала:

  panic   — процесс завершился ненулевым/аварийным кодом (паника Rust,
            access violation и т.п.)
  hang    — прогон не уложился в `--timeout` секунд
  white   — «белый экран»: DOM содержит >= WHITE_SCREEN_DOM_MIN узлов, но
            видимых единиц (текст/картинки/canvas/video/iframe) — 0. Повторяет
            эвристику `count_rendered_units`/`BROKEN_RENDER_DOM_MIN`
            (`crates/shell/src/main.rs`, PERF-6), т.к. `health.log`
            не пишет `broken_render` для headless dump-режима (только для
            окна с живым event-loop) — см. docs/tasks/p2-test-track.md.

На находке страница бисекционным минимизатором (ddmin по DOM-поддеревьям,
затем по CSS-правилам style-блока) сжимается до минимального репро с тем же
сигналом, репро сохраняется в `.tmp/page-fuzz/regressions/`. Инвариант, как
и у TEST-1: цель — отсутствие паник/хендж/white-screen на произвольном
почти-валидном входе, не корректность разбора/рендера.

Примеры:
  python scripts/page_fuzz.py --selftest                  # без браузера, в ворота
  python scripts/page_fuzz.py -n 300 --seed 1 --timeout 5  # прогон 300 страниц
  python scripts/page_fuzz.py -n 50 --profile release      # на release-сборке
  python scripts/page_fuzz.py --minimize .tmp/page-fuzz/regressions/panic_0007.html
"""

from __future__ import annotations

import argparse
import html.parser
import json
import os
import random
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Соответствует BROKEN_RENDER_DOM_MIN в crates/shell/src/health_log.rs.
WHITE_SCREEN_DOM_MIN = 20

# --------------------------------------------------------------------------
# Грамматика: теги/атрибуты/CSS-свойства из заведомо поддерживаемого движком
# подмножества. Источник тегов — BoxKind (crates/engine/layout/src/box_tree.rs,
# snapshot.rs) + UA-стили (style.rs); источник CSS-свойств — CSS-SPECS.md
# "Full Property Inventory", только строки со статусом ✅.
# --------------------------------------------------------------------------

BLOCK_TAGS = [
    "div", "section", "article", "header", "footer", "main", "aside", "nav",
    "p", "ul", "ol", "li", "blockquote", "figure", "figcaption",
    "h1", "h2", "h3", "h4", "h5", "h6", "form", "fieldset", "label",
    "table", "thead", "tbody", "tr", "td", "th",
]
INLINE_TAGS = [
    "span", "a", "strong", "em", "b", "i", "u", "small", "code", "mark",
    "sub", "sup", "abbr", "time", "q", "cite",
]
VOID_OR_REPLACED_TAGS = [
    "img", "br", "hr", "input", "button", "select", "option", "textarea",
    "video", "audio", "canvas", "iframe",
]
SVG_SHAPE_TAGS = ["rect", "circle", "ellipse", "line", "path"]

ALL_CONTAINER_TAGS = BLOCK_TAGS + INLINE_TAGS

TEXT_WORDS = [
    "lumen", "layout", "flex", "grid", "text", "box", "render", "test",
    "widget", "panel", "item", "value", "a", "the", "quick", "brown", "fox",
]


def rand_words(rng: random.Random, lo: int = 0, hi: int = 8) -> str:
    n = rng.randint(lo, hi)
    return " ".join(rng.choice(TEXT_WORDS) for _ in range(n))


# Value generators for CSS-SPECS.md ✅ properties. Each returns a plausible
# (not necessarily spec-perfect) value string. Kept intentionally "almost
# valid" per the TEST-2 brief — generative page fuzzers benefit from some
# malformed/edge values too, so a few generators intentionally include
# boundary cases (0, negative, huge numbers, empty string).

def g_length(rng: random.Random) -> str:
    unit = rng.choice(["px", "%", "em", "rem", "vw", "vh", "ch", ""])
    val = rng.choice([0, 1, 10, 100, 999, -5, 0.5, rng.randint(-50, 2000)])
    if unit == "" and val != 0:
        unit = "px"
    return f"{val}{unit}"


def g_color(rng: random.Random) -> str:
    return rng.choice([
        "red", "blue", "green", "black", "white", "transparent", "currentColor",
        f"#{rng.randrange(0, 0xFFFFFF):06x}",
        f"rgb({rng.randint(0,255)}, {rng.randint(0,255)}, {rng.randint(0,255)})",
        f"rgba({rng.randint(0,255)}, {rng.randint(0,255)}, {rng.randint(0,255)}, {rng.random():.2f})",
    ])


def g_keyword(*words):
    return lambda rng: rng.choice(words)


def g_calc(rng: random.Random) -> str:
    return rng.choice([
        g_length(rng),
        f"calc({g_length(rng)} + {g_length(rng)})",
        f"calc({g_length(rng)} - {g_length(rng)} * {rng.randint(1,5)})",
        f"min({g_length(rng)}, {g_length(rng)})",
        f"max({g_length(rng)}, {g_length(rng)})",
        f"clamp({g_length(rng)}, {g_length(rng)}, {g_length(rng)})",
    ])


CSS_PROPERTIES: list[tuple[str, "callable"]] = [
    ("display", g_keyword("block", "inline", "inline-block", "flex", "inline-flex",
                           "grid", "inline-grid", "none", "flow-root", "contents", "list-item")),
    ("position", g_keyword("static", "relative", "absolute", "fixed", "sticky")),
    ("width", g_calc),
    ("height", g_calc),
    ("min-width", g_calc),
    ("max-width", g_calc),
    ("min-height", g_calc),
    ("max-height", g_calc),
    ("margin", g_length),
    ("margin-top", g_length),
    ("margin-left", g_length),
    ("padding", g_length),
    ("padding-top", g_length),
    ("top", g_length),
    ("left", g_length),
    ("right", g_length),
    ("bottom", g_length),
    ("color", g_color),
    ("background-color", g_color),
    ("border-color", g_color),
    ("border-width", g_length),
    ("border-style", g_keyword("none", "solid", "dashed", "dotted", "double")),
    ("border-radius", g_length),
    ("overflow", g_keyword("visible", "hidden", "clip", "scroll", "auto")),
    ("overflow-x", g_keyword("visible", "hidden", "clip", "scroll", "auto")),
    ("overflow-y", g_keyword("visible", "hidden", "clip", "scroll", "auto")),
    ("z-index", lambda rng: str(rng.choice([0, 1, -1, 999, rng.randint(-100, 100), "auto"]))),
    ("opacity", lambda rng: f"{rng.random():.2f}"),
    ("visibility", g_keyword("visible", "hidden", "collapse")),
    ("box-sizing", g_keyword("content-box", "border-box")),
    ("flex-direction", g_keyword("row", "row-reverse", "column", "column-reverse")),
    ("flex-wrap", g_keyword("nowrap", "wrap", "wrap-reverse")),
    ("justify-content", g_keyword("flex-start", "flex-end", "center", "space-between",
                                   "space-around", "space-evenly")),
    ("align-items", g_keyword("stretch", "flex-start", "flex-end", "center", "baseline")),
    ("align-self", g_keyword("auto", "stretch", "flex-start", "flex-end", "center")),
    ("flex-grow", lambda rng: str(rng.choice([0, 1, 2, rng.randint(0, 10)]))),
    ("flex-shrink", lambda rng: str(rng.choice([0, 1, 2]))),
    ("flex-basis", g_calc),
    ("gap", g_length),
    ("grid-template-columns", lambda rng: " ".join(
        rng.choice(["1fr", "2fr", "100px", "auto", "minmax(50px, 1fr)"]) for _ in range(rng.randint(1, 4)))),
    ("grid-template-rows", lambda rng: " ".join(
        rng.choice(["1fr", "50px", "auto"]) for _ in range(rng.randint(1, 3)))),
    ("grid-column", lambda rng: f"{rng.randint(1,4)} / {rng.choice(['span 1', 'span 2', str(rng.randint(1,5))])}"),
    ("font-size", g_calc),
    ("font-weight", g_keyword("normal", "bold", "100", "400", "700", "900")),
    ("font-style", g_keyword("normal", "italic", "oblique")),
    ("line-height", lambda rng: rng.choice(["normal", "1", "1.5", "2", f"{g_length(rng)}"])),
    ("text-align", g_keyword("left", "right", "center", "justify", "start", "end")),
    ("text-decoration", g_keyword("none", "underline", "line-through", "overline")),
    ("text-transform", g_keyword("none", "uppercase", "lowercase", "capitalize")),
    ("text-overflow", g_keyword("clip", "ellipsis")),
    ("white-space", g_keyword("normal", "nowrap", "pre", "pre-wrap", "pre-line")),
    ("cursor", g_keyword("auto", "pointer", "default", "not-allowed", "grab")),
    ("transform", lambda rng: rng.choice([
        "none", f"scale({rng.uniform(0,3):.2f})", f"rotate({rng.randint(-360,360)}deg)",
        f"translate({g_length(rng)}, {g_length(rng)})", f"translateX({g_length(rng)})",
    ])),
    ("float", g_keyword("none", "left", "right")),
    ("clear", g_keyword("none", "left", "right", "both")),
]

# Small subset of DOM-mutation JS snippet templates, kept syntactically valid
# and self-contained (no network — file:// pages can't fetch, see CLAUDE.md
# gotchas). Placeholders: {sel} a CSS selector into the generated page.
JS_SNIPPET_TEMPLATES = [
    "document.querySelectorAll('{sel}').forEach(function(el){{ el.style.setProperty('opacity', '0.5'); }});",
    "var el = document.querySelector('{sel}'); if (el) {{ el.setAttribute('data-fuzz', '1'); }}",
    "var el = document.querySelector('{sel}'); if (el) {{ el.classList.toggle('fuzz'); }}",
    "var el = document.querySelector('{sel}'); if (el && el.parentNode) {{ el.parentNode.removeChild(el); }}",
    "var el = document.querySelector('{sel}'); if (el) {{ var c = document.createElement('div'); "
    "c.textContent = 'x'; el.appendChild(c); }}",
    "setTimeout(function(){{ var el = document.querySelector('{sel}'); if (el) {{ "
    "el.style.setProperty('display', 'none'); }} }}, 0);",
]


# --------------------------------------------------------------------------
# DOM model + generator
# --------------------------------------------------------------------------

@dataclass
class Node:
    """Минимальная DOM-модель, достаточная для генерации/сериализации/минимизации."""
    tag: str
    attrs: dict = field(default_factory=dict)
    style: dict = field(default_factory=dict)
    children: list = field(default_factory=list)
    text: str = ""
    node_id: str = ""  # заполняется при serialize() для JS-снипетов/минимизации


def make_node(rng: random.Random, tag: str, depth: int) -> Node:
    n = Node(tag=tag)
    if tag in ("a",):
        n.attrs["href"] = rng.choice(["#", "#section", "javascript:void(0)"])
    if tag == "img":
        n.attrs["src"] = rng.choice(["data:image/gif;base64,R0lGODlhAQABAAAAACw=", "missing.png"])
        n.attrs["alt"] = rand_words(rng, 0, 3)
        n.attrs["width"] = str(rng.choice([0, 1, 50, 400]))
        n.attrs["height"] = str(rng.choice([0, 1, 50, 400]))
    if tag == "input":
        n.attrs["type"] = rng.choice(["text", "checkbox", "radio", "number", "range", "hidden"])
        n.attrs["value"] = rand_words(rng, 0, 2)
    if tag == "canvas":
        n.attrs["width"] = str(rng.choice([0, 10, 300]))
        n.attrs["height"] = str(rng.choice([0, 10, 150]))
    if tag == "iframe":
        n.attrs["src"] = "about:blank"
    if tag in ("video", "audio"):
        n.attrs["controls"] = ""
    if rng.random() < 0.05:
        n.attrs["class"] = f"fuzz-{rng.randint(0, 3)}"
    if rng.random() < 0.3:
        nprops = rng.randint(1, 3)
        for prop, gen in rng.sample(CSS_PROPERTIES, min(nprops, len(CSS_PROPERTIES))):
            n.style[prop] = gen(rng)
    return n


def build_tree(rng: random.Random, max_depth: int, max_children: int) -> Node:
    root = Node(tag="body")

    def fill(node: Node, depth: int):
        if depth >= max_depth:
            return
        n_children = rng.randint(0, max_children)
        for _ in range(n_children):
            if rng.random() < 0.15:
                child = Node(tag=None, text=rand_words(rng, 1, 12))
                node.children.append(child)
                continue
            tag = rng.choice(ALL_CONTAINER_TAGS if depth < max_depth - 1 else
                              ALL_CONTAINER_TAGS + VOID_OR_REPLACED_TAGS)
            if rng.random() < 0.1:
                tag = rng.choice(VOID_OR_REPLACED_TAGS)
            child = make_node(rng, tag, depth)
            node.children.append(child)
            if tag not in VOID_OR_REPLACED_TAGS and tag not in ("br", "hr"):
                fill(child, depth + 1)

    fill(root, 0)
    return root


def collect_style_rules(rng: random.Random, root: Node, n_rules: int) -> list[str]:
    """Отдельный <style>-блок с классовыми правилами (независимо от inline-style узлов)."""
    rules = []
    for i in range(n_rules):
        sel = rng.choice([".fuzz-0", ".fuzz-1", ".fuzz-2", ".fuzz-3", "div", "span", "*"])
        nprops = rng.randint(1, 3)
        decls = []
        for prop, gen in rng.sample(CSS_PROPERTIES, min(nprops, len(CSS_PROPERTIES))):
            decls.append(f"{prop}: {gen(rng)};")
        rules.append(f"{sel} {{ {' '.join(decls)} }}")
    return rules


def serialize(root: Node, style_rules: list[str], scripts: list[str]) -> str:
    counter = [0]

    def esc(s: str) -> str:
        return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    def ser(node: Node) -> str:
        if node.tag is None:
            return esc(node.text)
        counter[0] += 1
        node.node_id = f"fz{counter[0]}"
        attrs = dict(node.attrs)
        attrs["id"] = node.node_id
        style_str = ";".join(f"{k}:{v}" for k, v in node.style.items())
        if style_str:
            attrs["style"] = style_str
        attr_str = "".join(f' {k}="{esc(str(v))}"' for k, v in attrs.items())
        if node.tag in ("img", "br", "hr", "input"):
            return f"<{node.tag}{attr_str}>"
        inner = "".join(ser(c) for c in node.children)
        return f"<{node.tag}{attr_str}>{inner}</{node.tag}>"

    body_html = "".join(ser(c) for c in root.children)
    style_block = "\n".join(style_rules)
    script_block = "\n".join(f"try {{ {s} }} catch (e) {{}}" for s in scripts)
    return (
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n"
        f"<style>\n{style_block}\n</style>\n</head>\n<body>\n{body_html}\n"
        f"<script>\n{script_block}\n</script>\n</body></html>\n"
    )


def expected_content_units(node: Node) -> int:
    """Non-whitespace text chars + visible-media tags planted by the generator itself.

    Mirrors the *source*-side half of the white-screen check: a page the
    generator never gave any text/media to is expected to render nothing, so
    a "white" classification for it is a false positive of the heuristic, not
    an engine defect (see white_screen_check / count_rendered_units parity
    note at module top). `display: none` on the node itself is excluded since
    the engine is then correctly not rendering it.
    """
    if node.tag is None:
        return sum(1 for ch in node.text if not ch.isspace())
    if node.style.get("display") == "none":
        return 0
    total = 1 if node.tag in ("img", "video", "canvas", "iframe") else 0
    for c in node.children:
        total += expected_content_units(c)
    return total


def generate_page(rng: random.Random, max_depth: int = 5, max_children: int = 4) -> tuple[str, Node, list[str]]:
    root = build_tree(rng, max_depth, max_children)
    style_rules = collect_style_rules(rng, root, rng.randint(0, 8))
    n_scripts = rng.randint(0, 3)
    scripts = []
    for _ in range(n_scripts):
        tpl = rng.choice(JS_SNIPPET_TEMPLATES)
        sel = rng.choice([".fuzz-0", ".fuzz-1", "div", "span", "*"])
        scripts.append(tpl.format(sel=sel))
    html_text = serialize(root, style_rules, scripts)
    return html_text, root, style_rules


# --------------------------------------------------------------------------
# Runner: invoke lumen.exe --dump-layout with a timeout, classify the result.
# --------------------------------------------------------------------------

@dataclass
class RunResult:
    returncode: int | None
    stdout: str
    stderr: str
    elapsed: float
    timed_out: bool


def find_lumen(profile: str | None, override: str | None) -> str:
    if override:
        return override
    profile = profile or os.environ.get("LUMEN_PROFILE", "dev-release")
    return os.path.join(REPO, "target", profile, "lumen.exe")


def run_lumen(exe: str, page_path: str, timeout: float) -> RunResult:
    start = time.time()
    try:
        proc = subprocess.run(
            [exe, "--dump-layout", page_path],
            capture_output=True, timeout=timeout, cwd=REPO, text=True, encoding="utf-8", errors="replace",
        )
        return RunResult(proc.returncode, proc.stdout, proc.stderr, time.time() - start, False)
    except subprocess.TimeoutExpired as e:
        stdout = e.stdout.decode("utf-8", "replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
        stderr = e.stderr.decode("utf-8", "replace") if isinstance(e.stderr, bytes) else (e.stderr or "")
        return RunResult(None, stdout, stderr, time.time() - start, True)


# Lines in serialize_layout_tree's text output that count as DOM/box presence.
_BOX_LINE_RE = re.compile(r"^\s*\w+ rect=\(")
_SEG_LINE_RE = re.compile(r'^\s*seg\[\d+\] "(.*)"', re.DOTALL)
_VISIBLE_KIND_RE = re.compile(r"^\s*(Image|Video|Canvas|Iframe) rect=\(")


def white_screen_check(stdout: str) -> tuple[bool, int, int]:
    """Реплика count_rendered_units/BROKEN_RENDER_DOM_MIN на тексте --dump-layout.

    Возвращает (is_white_screen, dom_box_count, rendered_units).
    """
    dom_boxes = 0
    rendered_units = 0
    for line in stdout.splitlines():
        if _BOX_LINE_RE.match(line):
            dom_boxes += 1
        if _VISIBLE_KIND_RE.match(line):
            rendered_units += 1
        m = _SEG_LINE_RE.match(line)
        if m:
            rendered_units += sum(1 for ch in m.group(1) if not ch.isspace())
    is_white = dom_boxes >= WHITE_SCREEN_DOM_MIN and rendered_units == 0
    return is_white, dom_boxes, rendered_units


def classify(result: RunResult) -> str:
    if result.timed_out:
        return "hang"
    if result.returncode == 0:
        is_white, _, _ = white_screen_check(result.stdout)
        return "white" if is_white else "ok"
    if result.returncode == 1:
        # Expected failure path (network/parse error surfaced as Err) — not a crash.
        return "error"
    if result.returncode == 101:
        return "panic"
    # Anything else (Windows abort/access-violation codes, signals on POSIX).
    return "crash"


FAILURE_KINDS = {"panic", "crash", "hang", "white"}


# --------------------------------------------------------------------------
# Minimizer: ddmin-style reduction over DOM children, then over style rules.
# --------------------------------------------------------------------------

def _flatten_reducible(node: Node) -> list[list[Node]]:
    """All child-lists in the tree (top-down), each a candidate for element removal."""
    out = [node.children]
    for c in node.children:
        if c.tag is not None:
            out.extend(_flatten_reducible(c))
    return out


def _all_positions(node: Node) -> list[tuple[list[Node], int]]:
    """(container_list, index) for every child in the tree, pre-order (parent before child)."""
    out = []
    for i, c in enumerate(node.children):
        out.append((node.children, i))
        if c.tag is not None:
            out.extend(_all_positions(c))
    return out


def minimize(root: Node, style_rules: list[str], check, max_rounds: int = 200) -> tuple[Node, list[str], int]:
    """Reduce root/style_rules while `check(html_text) -> bool` (still reproduces) holds.

    Three passes, repeated to a fixpoint (subtree removal <-> splice), then a
    final style-rule pass: (1) drop whole DOM subtrees; (2) splice a node out
    of the tree, promoting its own children into its parent's child list (this
    is what lets minimization collapse a marker buried under a chain of
    single-child wrapper elements down to just that marker — subtree removal
    alone can never do this, since removing a wrapper also removes whatever it
    wraps); (3) drop style rules one at a time. Simplified (not full
    binary-search) delta-debugging — good enough for a fuzz minimizer where
    re-runs are the expensive resource, not code elegance.
    """
    rounds = 0

    def html_now() -> str:
        return serialize(root, style_rules, [])

    outer_changed = True
    while outer_changed and rounds < max_rounds:
        outer_changed = False
        changed = True
        while changed and rounds < max_rounds:
            changed = False
            for child_list in _flatten_reducible(root):
                i = 0
                while i < len(child_list) and rounds < max_rounds:
                    removed = child_list.pop(i)
                    rounds += 1
                    if check(html_now()):
                        changed = True
                        outer_changed = True
                        # keep removed, don't increment i (list shifted)
                    else:
                        child_list.insert(i, removed)
                        i += 1

        changed = True
        while changed and rounds < max_rounds:
            changed = False
            for container, idx in _all_positions(root):
                if idx >= len(container) or container[idx].tag is None:
                    continue
                node = container[idx]
                replacement = node.children
                container[idx:idx + 1] = replacement
                rounds += 1
                if check(html_now()):
                    changed = True
                    outer_changed = True
                else:
                    container[idx:idx + 1] = [node]

    changed = True
    while changed and rounds < max_rounds:
        changed = False
        i = 0
        while i < len(style_rules) and rounds < max_rounds:
            removed = style_rules.pop(i)
            rounds += 1
            if check(html_now()):
                changed = True
            else:
                style_rules.insert(i, removed)
                i += 1
    return root, style_rules, rounds


# --------------------------------------------------------------------------
# Batch driver
# --------------------------------------------------------------------------

def run_batch(args) -> int:
    exe = find_lumen(args.profile, args.lumen)
    if not os.path.isfile(exe):
        print(f"page_fuzz: lumen binary not found at {exe} (build it first, or pass --lumen)", file=sys.stderr)
        return 2

    out_dir = args.out_dir
    reg_dir = os.path.join(out_dir, "regressions")
    os.makedirs(reg_dir, exist_ok=True)
    results_path = os.path.join(out_dir, "results.jsonl")

    base_seed = args.seed if args.seed is not None else int(time.time())
    counts: dict[str, int] = {}
    findings = []

    with open(results_path, "a", encoding="utf-8") as results_f:
        for i in range(args.count):
            seed_i = base_seed + i
            rng = random.Random(seed_i)
            html_text, root, style_rules = generate_page(rng, args.max_depth, args.max_children)
            page_path = os.path.join(out_dir, "current.html")
            with open(page_path, "w", encoding="utf-8") as f:
                f.write(html_text)

            result = run_lumen(exe, page_path, args.timeout)
            kind = classify(result)
            if kind == "white" and expected_content_units(root) == 0:
                # Generator planted no visible content on this page — rendering
                # nothing is correct, not a broken-render finding (see
                # expected_content_units docstring).
                kind = "white_empty"
            counts[kind] = counts.get(kind, 0) + 1

            record = {
                "seed": seed_i, "kind": kind, "elapsed": round(result.elapsed, 3),
                "returncode": result.returncode,
            }
            results_f.write(json.dumps(record) + "\n")
            results_f.flush()

            if kind in FAILURE_KINDS:
                idx = len(findings)
                raw_path = os.path.join(reg_dir, f"{kind}_{idx:04d}_seed{seed_i}.html")
                with open(raw_path, "w", encoding="utf-8") as f:
                    f.write(html_text)
                print(f"[{i+1}/{args.count}] FOUND {kind} (seed={seed_i}) -> {raw_path}")

                if args.minimize_findings:
                    def check(text: str, expect_kind: str = kind) -> bool:
                        tmp_path = os.path.join(out_dir, "minimize_probe.html")
                        with open(tmp_path, "w", encoding="utf-8") as tf:
                            tf.write(text)
                        r = run_lumen(exe, tmp_path, args.timeout)
                        return classify(r) == expect_kind

                    min_root, min_rules, rounds = minimize(root, style_rules, check)
                    min_html = serialize(min_root, min_rules, [])
                    min_path = os.path.join(reg_dir, f"{kind}_{idx:04d}_seed{seed_i}_min.html")
                    with open(min_path, "w", encoding="utf-8") as f:
                        f.write(min_html)
                    print(f"    minimized in {rounds} rounds -> {min_path}")

                findings.append({**record, "raw": raw_path})
            elif i % 25 == 0:
                print(f"[{i+1}/{args.count}] ok so far: {counts}")

    print(f"\npage_fuzz: {args.count} pages, {counts}")
    print(f"results log: {results_path}")
    if findings:
        print(f"{len(findings)} finding(s) — see {reg_dir}, file BUG-NNN per docs/tasks/p2-test-track.md")
    return 1 if findings else 0


def run_minimize_file(args) -> int:
    exe = find_lumen(args.profile, args.lumen)
    with open(args.minimize, "r", encoding="utf-8") as f:
        html_text = f.read()
    result = run_lumen(exe, args.minimize, args.timeout)
    kind = args.signature or classify(result)
    print(f"page_fuzz: reproducing signature {kind!r} for {args.minimize}")

    root = parse_existing_html(html_text)
    style_rules: list[str] = []

    def check(text: str) -> bool:
        tmp_path = os.path.join(os.path.dirname(args.minimize) or ".", "minimize_probe.html")
        with open(tmp_path, "w", encoding="utf-8") as tf:
            tf.write(text)
        r = run_lumen(exe, tmp_path, args.timeout)
        return classify(r) == kind

    if not check(serialize(root, style_rules, [])):
        print("page_fuzz: page does not reproduce the signature as-is (re-check --signature/--timeout)")
        return 2

    min_root, min_rules, rounds = minimize(root, style_rules, check)
    min_html = serialize(min_root, min_rules, [])
    out_path = args.minimize.rsplit(".html", 1)[0] + "_min.html"
    with open(out_path, "w", encoding="utf-8") as f:
        f.write(min_html)
    print(f"page_fuzz: minimized in {rounds} rounds -> {out_path}")
    return 0


class _BodyExtractor(html.parser.HTMLParser):
    """Very small best-effort parser: rebuilds a Node tree from <body> contents.

    Only used by --minimize on an already-generated (well-formed) page, not as
    a general HTML parser — good enough for re-minimizing our own output.
    """

    def __init__(self):
        super().__init__()
        self.root = Node(tag="body")
        self.stack = [self.root]
        self.in_body = False

    def handle_starttag(self, tag, attrs):
        if tag == "body":
            self.in_body = True
            return
        if not self.in_body or tag in ("style", "script", "head", "html", "meta"):
            return
        attrs_d = dict(attrs)
        style_d = {}
        if "style" in attrs_d:
            for decl in attrs_d.pop("style").split(";"):
                if ":" in decl:
                    k, v = decl.split(":", 1)
                    style_d[k.strip()] = v.strip()
        node = Node(tag=tag, attrs=attrs_d, style=style_d)
        self.stack[-1].children.append(node)
        if tag not in ("img", "br", "hr", "input"):
            self.stack.append(node)

    def handle_endtag(self, tag):
        if tag == "body":
            self.in_body = False
            return
        if len(self.stack) > 1 and self.stack[-1].tag == tag:
            self.stack.pop()

    def handle_data(self, data):
        if self.in_body and data.strip() and len(self.stack) > 1:
            self.stack[-1].children.append(Node(tag=None, text=data.strip()))


def parse_existing_html(html_text: str) -> Node:
    p = _BodyExtractor()
    p.feed(html_text)
    return p.root


# --------------------------------------------------------------------------
# Selftest: pure-Python checks, no lumen.exe / network needed.
# --------------------------------------------------------------------------

def run_selftest() -> int:
    ok = True

    # 1. Determinism: same seed -> byte-identical output.
    rng1 = random.Random(42)
    rng2 = random.Random(42)
    html1, _, _ = generate_page(rng1)
    html2, _, _ = generate_page(rng2)
    if html1 != html2:
        print("FAIL: generate_page is not deterministic for a fixed seed")
        ok = False
    else:
        print("OK: determinism (same seed -> identical page)")

    # 2. Well-formedness: generated pages parse without HTMLParser errors and
    #    every opened non-void tag has a matching close tag (stack-balance check).
    class BalanceChecker(html.parser.HTMLParser):
        def __init__(self):
            super().__init__()
            self.stack = []
            self.void = {"img", "br", "hr", "input", "meta"}
            self.balanced = True

        def handle_starttag(self, tag, attrs):
            if tag not in self.void:
                self.stack.append(tag)

        def handle_endtag(self, tag):
            if self.stack and self.stack[-1] == tag:
                self.stack.pop()
            else:
                self.balanced = False

    for seed in range(20):
        text, _, _ = generate_page(random.Random(seed))
        checker = BalanceChecker()
        try:
            checker.feed(text)
        except Exception as e:  # pragma: no cover - HTMLParser is lenient, but be safe
            print(f"FAIL: seed={seed} HTMLParser raised: {e}")
            ok = False
            continue
        if not checker.balanced or checker.stack:
            print(f"FAIL: seed={seed} generated page is not well-balanced (stack={checker.stack})")
            ok = False
    else:
        print("OK: 20 generated pages parse + balance-check clean")

    # 3. Minimizer converges to a single-node minimal repro on a synthetic
    #    predicate ("contains a node whose class is 'marker'"), without ever
    #    invoking lumen.exe.
    rng = random.Random(7)
    root = build_tree(rng, max_depth=4, max_children=4)
    marker = Node(tag="div", attrs={"class": "marker"})
    # graft the marker somewhere in the middle of the tree
    host = root
    while host.children and any(c.tag is not None and c.children for c in host.children):
        candidates = [c for c in host.children if c.tag is not None]
        if not candidates:
            break
        host = rng.choice(candidates)
    host.children.append(marker)
    style_rules = collect_style_rules(rng, root, 5)

    def has_marker(node: Node) -> bool:
        if node.attrs.get("class") == "marker":
            return True
        return any(has_marker(c) for c in node.children if c.tag is not None)

    def check(_text: str) -> bool:
        return has_marker(root)

    min_root, min_rules, rounds = minimize(root, style_rules, check, max_rounds=500)
    total_nodes = sum(1 for _ in _iter_nodes(min_root))
    if not has_marker(min_root):
        print("FAIL: minimizer lost the failing marker node")
        ok = False
    elif total_nodes > 2:  # body + marker is the theoretical minimum
        print(f"FAIL: minimizer left {total_nodes} nodes, expected close to 2 (rounds={rounds})")
        ok = False
    else:
        print(f"OK: minimizer reduced synthetic tree to {total_nodes} node(s) in {rounds} rounds")

    # 4. classify()/white_screen_check() smoke on synthetic dump-layout text.
    ok_stdout = 'Block rect=(0.00, 0.00, 100.00, 20.00)\n  InlineRun rect=(0.00, 0.00, 100.00, 20.00)\n    seg[0] "hello"\n'
    is_white, dom, units = white_screen_check(ok_stdout)
    if is_white or units == 0:
        print("FAIL: white_screen_check flagged a page with visible text")
        ok = False
    else:
        print("OK: white_screen_check does not flag text-bearing output")

    blank_lines = "\n".join(f"Block rect=({i}.00, 0.00, 10.00, 10.00)" for i in range(WHITE_SCREEN_DOM_MIN))
    is_white, dom, units = white_screen_check(blank_lines)
    if not is_white:
        print("FAIL: white_screen_check missed an actual white-screen case")
        ok = False
    else:
        print(f"OK: white_screen_check flags {dom} empty boxes / 0 units as white-screen")

    print("page_fuzz selftest: " + ("OK" if ok else "FAILED"))
    return 0 if ok else 1


def _iter_nodes(node: Node):
    yield node
    for c in node.children:
        if c.tag is not None:
            yield from _iter_nodes(c)


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------

def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("-n", "--count", type=int, default=100, help="pages to generate (default 100)")
    ap.add_argument("--seed", type=int, default=None, help="base RNG seed (default: current time)")
    ap.add_argument("--timeout", type=float, default=5.0, help="per-page timeout in seconds (default 5)")
    ap.add_argument("--max-depth", type=int, default=5, help="max DOM tree depth (default 5)")
    ap.add_argument("--max-children", type=int, default=4, help="max children per node (default 4)")
    ap.add_argument("--out-dir", default=os.path.join(REPO, ".tmp", "page-fuzz"),
                     help="scratch/output dir (default .tmp/page-fuzz)")
    ap.add_argument("--profile", default=None, help="lumen build profile (default $LUMEN_PROFILE or dev-release)")
    ap.add_argument("--lumen", default=None, help="explicit path to lumen.exe (overrides --profile)")
    ap.add_argument("--no-minimize", dest="minimize_findings", action="store_false", default=True,
                     help="skip auto-minimizing findings during a batch run (faster, keeps raw repro only)")
    ap.add_argument("--minimize", metavar="FILE", help="re-minimize a saved repro HTML file and exit")
    ap.add_argument("--signature", choices=sorted(FAILURE_KINDS), default=None,
                     help="expected failure kind for --minimize (default: whatever the file reproduces now)")
    ap.add_argument("--selftest", action="store_true", help="run offline self-test (no lumen.exe) and exit")
    args = ap.parse_args(argv)

    if args.selftest:
        return run_selftest()
    os.makedirs(args.out_dir, exist_ok=True)
    if args.minimize:
        return run_minimize_file(args)
    return run_batch(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
