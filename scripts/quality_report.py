#!/usr/bin/env python3
"""Сводный отчёт качества: WPT + graphic_tests + открытые баги с дельтами.

Сводит в один документ то, что раньше смотрели в четырёх местах: свежий и
предыдущий агрегат WPT (`docs/wpt/runs/*.json`, пишет `tests/wpt/run_corpus.py
--run-json`), два свежих прогона `graphic_tests/results/*.json`, открытые
BUG-NNN из `BUGS.md` и связи тест→баг (`tests/wpt/metadata/**/*.ini`,
`docs/wpt-status.md`, колонка `bugs` в `ROADMAP.md`). Раннеры не трогает —
только читает их выход.

  python scripts/quality_report.py            # docs/quality-report.md + docs/quality/runs/<stamp>.json
  python scripts/quality_report.py --check    # только итоговая строка в stdout, без записи

Выход идемпотентен: отчёт зависит только от данных, а JSON-срез пишется,
лишь если он отличается от последнего сохранённого. Нет источника или второго
прогона — в отчёте `n/a`, а не падение. Как читать — docs/quality-report-howto.md.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tests" / "wpt"))
import run_diff  # noqa: E402 — потестовое сравнение двух снимков (WPT-RUN-17)

NA = "n/a"

# Подсистемы — свод поверх верхнеуровневых категорий WPT. Агрегат хранит только
# категории верхнего уровня, поэтому `html/syntax` отдельно не выделить: строка
# HTML покрывает всю категорию `html`. Всё, что не перечислено, — «Остальные API».
SUBSYSTEMS = [
    ("HTML (в т.ч. парсер `html/syntax`)", ("html",)),
    ("DOM", ("dom", "shadow-dom", "custom-elements", "domparsing", "domxpath")),
    ("CSS layout", ("css",)),
    ("Сеть", ("fetch", "xhr", "cors")),
]
OTHER_SUBSYSTEM = "Остальные API"
RENDERING = "Рендеринг (reftest + crashtest)"
RENDER_TYPES = ("reftest", "crashtest")

# Канонические статусы уровня теста. PASS/FAIL дают только reftest и crashtest;
# testharness-тест завершается OK/ERROR/TIMEOUT/CRASH, а его прохождение — доля
# сабтестов. UNSUPPORTED — типы без исполнителя в раннере (coverage.no_executor),
# NOT-RUN — остальные непрогнанные. Ни один из них не попадает в числитель PASS.
STATUS_ORDER = ["PASS", "OK", "FAIL", "ERROR", "CRASH", "TIMEOUT",
                "UNSUPPORTED", "SKIPPED", "NOT-RUN", "OTHER"]
RAW_STATUS = {"PASS": "PASS", "OK": "OK", "FAIL": "FAIL", "ERROR": "ERROR",
              "CRASH": "CRASH", "TIMEOUT": "TIMEOUT", "SKIP": "SKIPPED",
              "NOT-RUN": "NOT-RUN"}

GT_STATUS_ORDER = ["PASS", "FAIL", "ERROR", "DEBTOR", "SKIP"]
GT_SUMMARY_KEY = {"PASS": "passed", "FAIL": "failed", "ERROR": "errors",
                  "DEBTOR": "debtors", "SKIP": "skipped"}

# Порог «просадки» категории WPT: балл упал и потерян хотя бы один id-эквивалент
# балла или хотя бы один тест, завершавшийся OK/PASS. Потеря OK без потери балла
# (тест без пройденных сабтестов стал TIMEOUT) — шум, не просадка.
REGRESSION_SCORE = -1.0
# Параметры раннера WPT, от которых зависят TIMEOUT/CRASH: разные значения у двух
# прогонов — разница может быть от окружения, а не от кода.
RUNNER_KEYS = ("processes", "parallel_shards", "batch_small", "shared_queue", "no_paint",
               "max_browser_gb", "exclude_prefixes")
# Сколько худших тестов выводить под каждой просевшей категорией WPT.
WORST_TESTS = 10
# Источники, разнесённые во времени дальше этого, описывают разные состояния кода.
STALE_DAYS = 7
# Каталог из ROADMAP шире этого числа тестов — не привязка к багу, а область.
ROADMAP_DIR_CAP = 500
TEST_EXT = (".html", ".htm", ".xht", ".xhtml", ".svg", ".js")
NON_TEST_PARTS = {"support", "resources", "reference", "references", "tools", "common"}


# ---------------------------------------------------------------- utilities

def load_json(path: Path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def read_md(path: Path) -> str:
    # newline='' — BUGS.md хранит голые CR внутри ячеек (CLAUDE.md §gotchas).
    with open(path, encoding="utf-8", newline="") as f:
        return f.read()


def pct(x) -> str:
    return NA if x is None else f"{x * 100:.2f}%"


def signed(x, digits: int = 0, unit: str = "") -> str:
    if x is None:
        return NA
    if digits:
        s = f"{x:+.{digits}f}"
        if float(s) == 0:
            s = f"{0:.{digits}f}"
    else:
        s = f"{x:+d}" if x else "0"
    return s + unit


def pp(cur, prev) -> str:
    """Дельта долей в процентных пунктах."""
    if cur is None or prev is None:
        return NA
    return signed((cur - prev) * 100, 2, " п.п.")


def diff(cur, prev):
    return None if cur is None or prev is None else cur - prev


def days_between(a: str | None, b: str | None) -> int | None:
    """Целых суток между двумя ISO-метками (знак не важен)."""
    if not a or not b:
        return None
    try:
        return abs((datetime.fromisoformat(a) - datetime.fromisoformat(b)).days)
    except ValueError:
        return None


def commit_lag(root: Path, commit: str | None) -> str:
    """Насколько коммит прогона отстаёт от `origin/main` (нет его — от `main`):
    «N коммитов» или «вне main» (коммит невлитой ветки). Нет git или коммита — n/a."""
    if not commit:
        return NA
    try:
        for ref in ("origin/main", "main"):
            rc = subprocess.run(["git", "merge-base", "--is-ancestor", commit, ref], cwd=root,
                                capture_output=True, timeout=20).returncode
            if rc == 1:
                return "вне main"
            if rc == 0:
                n = subprocess.run(["git", "rev-list", "--count", f"{commit}..{ref}"], cwd=root,
                                   capture_output=True, text=True, timeout=20)
                return f"{n.stdout.strip()} коммитов" if n.returncode == 0 else NA
    except (OSError, subprocess.SubprocessError):
        pass
    return NA


# ---------------------------------------------------------------- WPT

def wpt_runs(runs_dir: Path) -> list:
    """Полные прогоны корпуса (scope == full-corpus), от старого к свежему.

    Частичные прогоны (scope — список категорий) с полным корпусом не
    сравнимы: их знаменатель — несколько категорий. Снимок, названный в
    `derived_from` другого снимка (составной: исходные отчёты + перепрогон
    части шардов, как «с поддоменами» у WPT-RUN-9), заменён им и выпадает —
    иначе свежая пара — два снимка одного коммита, а не два прогона.
    """
    runs = []
    for path in sorted(runs_dir.glob("*.json")) if runs_dir.is_dir() else []:
        try:
            data = load_json(path)
        except (OSError, ValueError):
            continue
        if data.get("scope") != "full-corpus" or "scored" not in data:
            continue
        runs.append((data.get("finished", ""), path.name, data))
    replaced = {r[2].get("derived_from") for r in runs}
    runs.sort(key=lambda r: (r[0], r[1]))
    return [(name, data) for _, name, data in runs if name not in replaced]


def wpt_statuses(scored: dict) -> dict:
    raw = scored.get("status_counts", {})
    out = dict.fromkeys(STATUS_ORDER, 0)
    for status, n in raw.items():
        out[RAW_STATUS.get(status, "OTHER")] += n
    unsupported = min(scored.get("coverage", {}).get("no_executor", 0), out["NOT-RUN"])
    out["UNSUPPORTED"] = unsupported
    out["NOT-RUN"] -= unsupported
    return out


def subsystem_of(category: str) -> str:
    for name, cats in SUBSYSTEMS:
        if category in cats:
            return name
    return OTHER_SUBSYSTEM


def wpt_summary(name: str, data: dict) -> dict:
    scored = data["scored"]
    per_cat = scored.get("per_category", {})
    subs = {}
    for cat, row in per_cat.items():
        acc = subs.setdefault(subsystem_of(cat), dict.fromkeys(
            ("ids", "ran", "harness_ok", "score", "subtests_passed", "subtests_total"), 0))
        for key in acc:
            acc[key] += row.get(key, 0)
    for acc in subs.values():
        acc["score"] = round(acc["score"], 2)
        acc["pass_rate"] = round(acc["score"] / acc["ids"], 4) if acc["ids"] else None
    statuses = wpt_statuses(scored)
    render_ids = sum(row.get("by_type", {}).get(t, 0) for row in per_cat.values() for t in RENDER_TYPES)
    rendering = {"ids": render_ids, "pass": statuses["PASS"], "fail": statuses["FAIL"],
                 "pass_rate": round(statuses["PASS"] / render_ids, 4) if render_ids else None}
    totals = scored.get("totals", {})
    sub_total = totals.get("subtests_total", 0)
    return {
        "run": name,
        "commit": data.get("commit"),
        "finished": data.get("finished"),
        "runner": {k: data[k] for k in RUNNER_KEYS if k in data},
        "totals": totals,
        "subtest_rate": round(totals.get("subtests_passed", 0) / sub_total, 4) if sub_total else None,
        "statuses": statuses,
        "status_sum_ok": sum(statuses.values()) == totals.get("ids"),
        "subsystems": subs,
        "rendering": rendering,
        "categories": {cat: {k: row.get(k) for k in ("ids", "ran", "harness_ok", "score", "pass_rate")}
                       for cat, row in sorted(per_cat.items())},
    }


def wpt_regressions(cur: dict, prev: dict) -> list:
    rows = []
    for cat, row in cur["categories"].items():
        old = prev["categories"].get(cat)
        if not old:
            continue
        d_score = round(row["score"] - old["score"], 2)
        d_ok = row["harness_ok"] - old["harness_ok"]
        if d_score < 0 and (d_score <= REGRESSION_SCORE or d_ok < 0):
            rows.append((cat, d_score, d_ok, row, old))
    rows.sort(key=lambda r: (r[1], r[2]))
    return rows


def wpt_worst(runs_dir: Path, cur: tuple, prev: tuple, regressions: list) -> dict:
    """Худшие тесты каждой просевшей категории: `{категория: [запись run_diff]}`.

    Нужны оба снимка с `ids_file` (`run_corpus.py --run-json` пишет его рядом
    со снимком): агрегаты говорят, что категория просела, а какой тест — только
    потестовый файл. Если у любого из двух его нет — пусто, отчёт как раньше.
    """
    tables = []
    for name, data in (prev, cur):
        ids = data.get("ids_file")
        path = runs_dir / ids if ids else None
        if not path or not path.is_file():
            return {}
        tables.append(run_diff.ids_file.read(str(path)))
    return {cat: run_diff.worst_tests(tables[0], tables[1], cat, WORST_TESTS)
            for cat, *_ in regressions}


def runner_diff(cur: dict, prev: dict) -> list:
    """Параметры раннера, различающиеся у двух прогонов: `processes` 14 → 7."""
    a, b = prev.get("runner", {}), cur.get("runner", {})
    return [f"`{k}` {a.get(k, NA)} → {b.get(k, NA)}" for k in RUNNER_KEYS
            if (k in a or k in b) and a.get(k) != b.get(k)]


def same_commit(cur: dict | None, prev: dict | None) -> bool:
    """Оба прогона сняты на одном коммите: их разница — окружение и разброс, не код."""
    return bool(cur and prev and cur.get("commit") and cur["commit"] == prev.get("commit"))


# ---------------------------------------------------------------- graphic_tests

def graphic_runs(results_dir: Path) -> list:
    """Полные прогоны graphic_tests, от старого к свежему.

    `latest.json`/`baselines.json` — служебные; прогон короче 90 % самого
    длинного из последних 20 считается частичным (`--only`, `--recheck`).
    """
    runs = []
    for path in sorted(results_dir.glob("[0-9]*.json")) if results_dir.is_dir() else []:
        try:
            data = load_json(path)
        except (OSError, ValueError):
            continue
        if not isinstance(data, dict) or "tests" not in data or "summary" not in data:
            continue
        runs.append((data.get("timestamp", ""), path.name, data))
    runs.sort(key=lambda r: (r[0], r[1]))
    if not runs:
        return []
    longest = max(len(r[2]["tests"]) for r in runs[-20:])
    return [(name, data) for _, name, data in runs if len(data["tests"]) >= 0.9 * longest]


def graphic_mode(data: dict) -> str:
    """Способ съёмки прогона: `ipc` / `gdigrab` (все тесты одним способом),
    `default` (смесь по `GPU_CAPTURE` из `graphic_tests/run.py`) и `legacy` —
    прогон до TEST-9, у тестов нет поля `capture`."""
    caps = {t.get("capture") for t in data["tests"]}
    if caps == {None}:
        return "legacy"
    if len(caps) == 1:
        return caps.pop()
    return "default"


def graphic_pair(runs: list) -> tuple:
    """(свежий, предыдущий того же режима | None, причина отсутствия | None).

    Прогоны разных способов съёмки несопоставимы (CPU-снимок IPC и gdigrab
    живого окна расходятся на AA-краях), поэтому предыдущий — последний более
    ранний полный прогон **того же режима**.
    """
    if not runs:
        return None, None, None
    cur = runs[-1]
    mode = graphic_mode(cur[1])
    for prev in reversed(runs[:-1]):
        if graphic_mode(prev[1]) == mode:
            return cur, prev, None
    others = sorted({graphic_mode(d) for _, d in runs[:-1]})
    why = f"нет более раннего полного прогона режима `{mode}`"
    if others:
        why += f" (есть только режимы {', '.join(f'`{m}`' for m in others)} — несопоставимы)"
    return cur, None, why


def git_origin(root: Path | None, git: dict) -> dict:
    """Пометки прогона: `wip` — коммит с темой `WIP…`; `in_main` — коммит предок
    `origin/main` (None, если git недоступен или коммит ему неизвестен)."""
    wip = str(git.get("subject") or "").lstrip().upper().startswith("WIP")
    in_main = None
    commit = git.get("commit")
    if root is not None and commit:
        try:
            for ref in ("origin/main", "main"):
                rc = subprocess.run(["git", "merge-base", "--is-ancestor", commit, ref], cwd=root,
                                    capture_output=True, timeout=20).returncode
                if rc in (0, 1):
                    in_main = rc == 0
                    break
        except (OSError, subprocess.SubprocessError):
            in_main = None
    return {"in_main": in_main, "wip": wip}


def origin_mark(origin: dict | None) -> str:
    if not origin:
        return ""
    if origin.get("in_main") is False:
        return "не в main / WIP" if origin.get("wip") else "не в main"
    return "WIP" if origin.get("wip") else ""


def graphic_summary(name: str, data: dict, root: Path | None = None) -> dict:
    counts = dict.fromkeys(GT_STATUS_ORDER, 0)
    tests = {}
    for t in data["tests"]:
        status = t.get("status", "")
        counts[status] = counts.get(status, 0) + 1
        tests[t.get("stem") or t.get("id")] = {"status": status, "diff_pct": t.get("diff_pct"),
                                               "capture": t.get("capture")}
    summary = data.get("summary", {})
    mismatch = [s for s, key in GT_SUMMARY_KEY.items() if summary.get(key, 0) != counts.get(s, 0)]
    total = len(data["tests"])
    git = data.get("git", {})
    return {
        "run": name,
        "timestamp": data.get("timestamp"),
        "commit": git.get("commit"),
        "branch": git.get("branch"),
        "mode": graphic_mode(data),
        "origin": git_origin(root, git),
        "total": total,
        "counts": counts,
        "pass_rate": round(counts["PASS"] / total, 4) if total else None,
        "summary_mismatch": mismatch,
        "tests": tests,
    }


def graphic_changes(cur: dict, prev: dict) -> list:
    """Сменившие статус тесты. Тест, у которого в двух прогонах разный способ
    съёмки, сюда не попадает — см. `graphic_capture_changes`."""
    out = []
    for stem, t in cur["tests"].items():
        old = prev["tests"].get(stem)
        if old is None:
            out.append((stem, "—", t["status"], t["diff_pct"], None))
        elif old.get("capture") != t.get("capture"):
            continue
        elif old["status"] != t["status"]:
            out.append((stem, old["status"], t["status"], t["diff_pct"], old["diff_pct"]))
    for stem, old in prev["tests"].items():
        if stem not in cur["tests"]:
            out.append((stem, old["status"], "—", None, old["diff_pct"]))
    return sorted(out)


def graphic_capture_changes(cur: dict, prev: dict) -> list:
    """Тесты, снятые в двух прогонах разными способами: (stem, было, стало)."""
    return sorted((stem, prev["tests"][stem].get("capture"), t.get("capture"))
                  for stem, t in cur["tests"].items()
                  if stem in prev["tests"] and prev["tests"][stem].get("capture") != t.get("capture"))


def graphic_is_regression(change) -> bool:
    _, old, new, _, _ = change
    return old == "PASS" and new not in ("PASS", "—")


# ---------------------------------------------------------------- bugs

BUG_ROW = re.compile(r"^\|\s*\[?(BUG-\d+)\]?[^|]*\|([^|]*)\|([^|]*)\|")
BUG_ID = re.compile(r"BUG-(\d+)")


def is_open(status: str) -> bool:
    s = status.strip().upper()
    return s.startswith("OPEN") or s.startswith("IN PROGRESS")


def components(raw: str) -> list:
    """`layout/js (`crates/…`)` → [layout, js]: пояснение после скобки/кода/тире отбрасывается."""
    head = re.split(r"\s*[(`—:]", raw.strip(), maxsplit=1)[0]
    parts = [p.strip().lower() for p in re.split(r"[/,;+]", head)]
    return [p for p in parts if p] or ["(не указан)"]


def open_bugs(bugs_md: Path) -> dict:
    """{BUG-NNN: {"status", "components"}} — только OPEN / IN PROGRESS."""
    if not bugs_md.is_file():
        return {}
    out = {}
    for line in read_md(bugs_md).split("\n"):
        m = BUG_ROW.match(line)
        if m and is_open(m.group(2)):
            out[m.group(1)] = {"status": m.group(2).strip(), "components": components(m.group(3))}
    return out


def is_test_path(rel: str) -> bool:
    parts = rel.replace("\\", "/").split("/")
    name = parts[-1]
    if not name.endswith(TEST_EXT) or NON_TEST_PARTS.intersection(parts[:-1]):
        return False
    return not re.search(r"-(not)?ref\.|-ref$|^ref-", name)


def bugs_from_metadata(meta_dir: Path, open_ids: set) -> dict:
    """{BUG: {test}} по `.ini` ожиданиям: файл ожиданий называет баг, держащий тест."""
    out = {}
    if not meta_dir.is_dir():
        return out
    for ini in meta_dir.rglob("*.ini"):
        if ini.name == "__dir__.ini":
            continue
        try:
            text = ini.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if "BUG-" not in text:
            continue
        test = "/" + ini.relative_to(meta_dir).as_posix()[:-len(".ini")]
        for num in set(BUG_ID.findall(text)):
            bug = f"BUG-{num}"
            if bug in open_ids:
                out.setdefault(bug, set()).add(test)
    return out


def bugs_from_wpt_status(status_md: Path, open_ids: set, wpt: dict | None) -> tuple:
    """(доля категорий {BUG: float}, тесты {BUG: {test}}) из таблиц docs/wpt-status.md.

    Строка категорийного индекса связывает баг с категорией целиком: её недобор
    (тесты, не завершившиеся OK/PASS, по свежему прогону) делится поровну между
    открытыми багами строки. Строка детальной таблицы (`/dom/nodes/…`) — один
    тест, если его сабтесты пройдены не все.
    """
    shares, tests = {}, {}
    if not status_md.is_file():
        return shares, tests
    cats = wpt["categories"] if wpt else {}
    for line in read_md(status_md).split("\n"):
        cells = [c.strip() for c in line.split("|")]
        if len(cells) < 7 or not cells[1].startswith("`"):
            continue
        key = cells[1].strip("`")
        bugs = sorted({f"BUG-{n}" for n in BUG_ID.findall(cells[5])} & open_ids)
        if not bugs:
            continue
        if key.startswith("/"):
            m = re.match(r"(\d+)/(\d+)$", cells[3])
            if m and m.group(1) == m.group(2) and cells[2] in ("OK", "PASS"):
                continue
            for bug in bugs:
                tests.setdefault(bug, set()).add(key)
        elif key in cats:
            row = cats[key]
            shortfall = (row["ids"] or 0) - (row["harness_ok"] or 0)
            for bug in bugs:
                shares[bug] = shares.get(bug, 0.0) + shortfall / len(bugs)
    return shares, tests


def resolve_wpt_token(token: str, wpt_dir: Path, cache: dict) -> set:
    """Тесты под путём WPT из текста ROADMAP (`css-shapes/shape-outside`, `navigation/pagereveal-*`)."""
    token = token.strip().strip("/")
    if (not token or " " in token or ".." in token or "/" not in token
            or token.startswith(("crates/", "docs/", "scripts/", "tests/", "samples/"))):
        return set()
    if token in cache:
        return cache[token]
    found = set()
    for base in (wpt_dir, wpt_dir / "css"):
        if "*" in token:
            matches = list(base.glob(token))
        else:
            p = base / token
            matches = [p] if p.exists() else []
        for p in matches:
            if p.is_file():
                files = [p]
            else:
                files = [f for f in p.rglob("*") if f.is_file()]
            rels = {"/" + f.relative_to(wpt_dir).as_posix() for f in files}
            rels = {r for r in rels if is_test_path(r)}
            if p.is_dir() and len(rels) > ROADMAP_DIR_CAP:
                continue
            found |= rels
        if found:
            break
    cache[token] = found
    return found


def bugs_from_roadmap(roadmap_md: Path, wpt_dir: Path, open_ids: set) -> tuple:
    """(тесты {BUG: {test}}, задачи {BUG: [id]}) из незакрытых строк ROADMAP с колонкой bugs."""
    tests, tasks = {}, {}
    if not roadmap_md.is_file():
        return tests, tasks
    cache = {}
    header = None
    for line in read_md(roadmap_md).split("\n"):
        if not line.startswith("|"):
            header = None
            continue
        cells = [c.strip() for c in line.split("|")]
        if "bugs" in cells and "status" in cells:
            header = cells
            continue
        if header is None or len(cells) != len(header) or cells[1].startswith("---"):
            continue
        row = dict(zip(header, cells))
        if row.get("status") == "done":
            continue
        bugs = sorted({f"BUG-{n}" for n in BUG_ID.findall(row.get("bugs", ""))} & open_ids)
        if not bugs:
            continue
        cited = set()
        for token in re.findall(r"`([^`]+)`", row.get("note", "")):
            cited |= resolve_wpt_token(token, wpt_dir, cache)
        for bug in bugs:
            tasks.setdefault(bug, []).append(row.get("id", "?"))
            if cited:
                tests.setdefault(bug, set()).update(cited)
    return tests, tasks


def top_bugs(bugs: dict, meta: dict, status_tests: dict, shares: dict,
             roadmap_tests: dict, tasks: dict, limit: int = 5) -> list:
    rows = []
    for bug in set(meta) | set(status_tests) | set(shares) | set(roadmap_tests):
        exact = meta.get(bug, set()) | status_tests.get(bug, set()) | roadmap_tests.get(bug, set())
        share = round(shares.get(bug, 0.0))
        rows.append({
            "bug": bug,
            "blocked": len(exact) + share,
            "metadata": len(meta.get(bug, ())),
            "roadmap": len(roadmap_tests.get(bug, ())),
            "wpt_status": len(status_tests.get(bug, ())) + share,
            "components": bugs.get(bug, {}).get("components", []),
            "tasks": tasks.get(bug, []),
        })
    rows.sort(key=lambda r: (-r["blocked"], int(r["bug"][4:])))
    return rows[:limit]


def bugs_summary(root: Path, wpt: dict | None) -> dict | None:
    bugs_md = root / "BUGS.md"
    if not bugs_md.is_file():
        return None
    bugs = open_bugs(bugs_md)
    ids = set(bugs)
    by_comp = {}
    for info in bugs.values():
        for comp in info["components"]:
            by_comp[comp] = by_comp.get(comp, 0) + 1
    meta = bugs_from_metadata(root / "tests" / "wpt" / "metadata", ids)
    shares, status_tests = bugs_from_wpt_status(root / "docs" / "wpt-status.md", ids, wpt)
    roadmap_tests, tasks = bugs_from_roadmap(root / "ROADMAP.md", root / "tests" / "wpt", ids)
    return {
        "open": len(bugs),
        "rework": sum(1 for b in bugs.values() if "ДОРАБОТКА" in b["status"].upper()),
        "debtor": sum(1 for b in bugs.values() if "DEBTOR" in b["status"].upper()),
        "by_component": dict(sorted(by_comp.items(), key=lambda kv: (-kv[1], kv[0]))),
        "top5": top_bugs(bugs, meta, status_tests, shares, roadmap_tests, tasks),
    }


# ---------------------------------------------------------------- snapshots

def snapshot_stamp(snap: dict) -> str:
    """Имя среза — время самого свежего источника, а не время запуска."""
    stamps = [s for s in (snap["wpt"] and snap["wpt"]["finished"],
                          snap["graphic"] and snap["graphic"]["timestamp"]) if s]
    if not stamps:
        return "00000000-000000"
    return datetime.fromisoformat(max(stamps)).strftime("%Y%m%d-%H%M%S")


def snapshot_payload(snap: dict) -> dict:
    """Срез без длинных хвостов: история нужна для дельт, не для полного отчёта."""
    out = json.loads(json.dumps(snap, default=sorted))
    if out["graphic"]:
        out["graphic"]["tests"] = {k: v["status"] for k, v in out["graphic"]["tests"].items()}
        # Зависит от состояния git на момент отчёта, а не от прогона: срез не должен дрожать от мержей.
        out["graphic"].pop("origin", None)
    return out


def snapshot_order(path: Path) -> tuple:
    """`<stamp>.json` < `<stamp>-2.json` < `<stamp>-3.json`: по строке `-2` встал бы раньше `.json`."""
    n = path.stem[16:]
    return path.stem[:15], int(n) if n.isdigit() else 1


def previous_snapshot(runs_dir: Path, payload: dict) -> tuple:
    """(срез для дельты, нужно ли писать новый): равный последнему не пишется,
    а дельта считается к тому, что был до него."""
    snaps = sorted(runs_dir.glob("*.json"), key=snapshot_order) if runs_dir.is_dir() else []
    loaded = []
    for path in snaps:
        try:
            loaded.append(load_json(path))
        except (OSError, ValueError):
            continue
    if loaded and loaded[-1] == payload:
        return (loaded[-2] if len(loaded) > 1 else None), False
    return (loaded[-1] if loaded else None), True


# ---------------------------------------------------------------- report

def source_gap(snap: dict) -> int | None:
    """На сколько суток свежий прогон WPT разошёлся со свежим graphic_tests."""
    wpt, gt = snap["wpt"], snap["graphic"]
    return days_between(wpt and wpt["finished"], gt and gt["timestamp"])


def headline(snap: dict, prev_snap: dict | None, regressions: int | None) -> str:
    wpt, wpt_prev, gt, gt_prev = snap["wpt"], snap["wpt_prev"], snap["graphic"], snap["graphic_prev"]
    parts = []
    if wpt:
        rate = wpt["totals"].get("pass_rate")
        note = ", тот же коммит — разброс" if same_commit(wpt, wpt_prev) else ""
        parts.append(f"WPT {pct(rate)} (Δ {pp(rate, wpt_prev and wpt_prev['totals'].get('pass_rate'))}{note})")
    else:
        parts.append(f"WPT {NA}")
    if gt:
        d = diff(gt["counts"]["PASS"], gt_prev and gt_prev["counts"]["PASS"])
        parts.append(f"graphic_tests {gt['counts']['PASS']}/{gt['total']} PASS (Δ {signed(d)})")
    else:
        parts.append(f"graphic_tests {NA}")
    if snap["bugs"]:
        prev_open = prev_snap["bugs"]["open"] if prev_snap and prev_snap.get("bugs") else None
        parts.append(f"открытых багов {snap['bugs']['open']} (Δ {signed(diff(snap['bugs']['open'], prev_open))})")
    else:
        parts.append(f"открытых багов {NA}")
    parts.append(f"просадок {NA if regressions is None else regressions}")
    gap = source_gap(snap)
    if gap is not None and gap > STALE_DAYS:
        older = "WPT" if wpt["finished"] < gt["timestamp"] else "graphic_tests"
        parts.append(f"{older} старше соседнего источника на {gap} дн.")
    return " · ".join(parts)


def md_table(header: list, rows: list) -> list:
    out = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    out += ["| " + " | ".join(str(c) for c in row) + " |" for row in rows]
    return out


def render(snap: dict, prev_snap: dict | None) -> tuple:
    wpt, wpt_prev = snap["wpt"], snap["wpt_prev"]
    gt, gt_prev = snap["graphic"], snap["graphic_prev"]
    bugs = snap["bugs"]
    wpt_regs = wpt_regressions(wpt, wpt_prev) if wpt and wpt_prev else None
    gt_changes = graphic_changes(gt, gt_prev) if gt and gt_prev else None
    gt_cap = graphic_capture_changes(gt, gt_prev) if gt and gt_prev else []
    gt_regs = [c for c in gt_changes if graphic_is_regression(c)] if gt_changes is not None else None
    status_regs = []
    if wpt and wpt_prev:
        for s in ("CRASH", "TIMEOUT", "ERROR", "FAIL"):
            d = wpt["statuses"][s] - wpt_prev["statuses"][s]
            if d > 0:
                status_regs.append((s, d))
    # Два прогона одного коммита код не сравнивают: их «просадки» — разброс.
    wpt_noise = same_commit(wpt, wpt_prev)
    n_regs = None
    if wpt_regs is not None or gt_regs is not None:
        n_regs = len(gt_regs or []) + (0 if wpt_noise else len(wpt_regs or []) + len(status_regs))
    head = headline(snap, prev_snap, n_regs)

    def wpt_drop_lines() -> list:
        out = [f"- WPT: тестов со статусом {s} стало больше на {d} "
               f"({wpt_prev['statuses'][s]} → {wpt['statuses'][s]})." for s, d in status_regs]
        for cat, d_score, d_ok, row, old in (wpt_regs or [])[:25]:
            out.append(f"- WPT `{cat}`: балл {old['score']} → {row['score']} ({signed(d_score, 2)}), "
                       f"OK/PASS {old['harness_ok']} → {row['harness_ok']} ({signed(d_ok)}), "
                       f"pass rate {pp(row['pass_rate'], old['pass_rate'])}")
            for t in (snap.get("wpt_worst") or {}).get(cat, []):
                out.append(f"  - `{t['id']}`: {run_diff.transition(t)} ({signed(t['delta'], 2)})")
        if wpt_regs and len(wpt_regs) > 25:
            out.append(f"- … и ещё {len(wpt_regs) - 25} категорий WPT (полная таблица — в конце).")
        return out

    L = ["# Отчёт качества", "",
         "<!-- Генерируется `python scripts/quality_report.py` — руками не править. "
         "Как читать — docs/quality-report-howto.md -->", "",
         f"**Итог:** {head}", ""]

    # Источники
    L += ["## Источники", ""]
    lag = snap.get("lag", {})
    src = []
    if wpt:
        prev_txt = f"`{wpt_prev['run']}` · `{wpt_prev['commit']}` · {wpt_prev['finished']}" if wpt_prev else NA
        src.append(("WPT (`docs/wpt/runs`)", f"`{wpt['run']}` · `{wpt['commit']}` · {wpt['finished']}", prev_txt,
                    lag.get("wpt", NA)))
    else:
        src.append(("WPT (`docs/wpt/runs`)", NA, NA, NA))
    if gt:
        def gt_cell(g: dict) -> str:
            cell = f"`{g['run']}` · `{g['commit']}` ({g['branch']}) · режим `{g['mode']}`"
            mark = origin_mark(g.get("origin"))
            return f"{cell} · **{mark}**" if mark else cell

        src.append(("graphic_tests", gt_cell(gt), gt_cell(gt_prev) if gt_prev else NA, lag.get("graphic", NA)))
    else:
        src.append(("graphic_tests", NA, NA, NA))
    bug_prev = f"срез `{prev_snap['stamp']}`" if prev_snap and prev_snap.get("bugs") else NA
    src.append(("BUGS.md", "рабочее дерево" if bugs else NA, bug_prev, "—"))
    L += md_table(["Источник", "Свежий", "Предыдущий", "Отставание от main"], src) + [""]
    L += ["Дельта WPT — к предыдущему прогону; дельта graphic_tests — к предыдущему полному прогону "
          "**того же режима съёмки** (`ipc` / `gdigrab` / `default` — смесь по `GPU_CAPTURE`; `legacy` — "
          "прогон до TEST-9 без поля `capture`); дельта багов — к прошлому срезу `docs/quality/runs/`. "
          "Частичные прогоны WPT (scope — список категорий) и graphic_tests (`--only`, `--recheck`) "
          "в сравнение не берутся. «не в main» — коммит прогона не предок `origin/main`; "
          "«WIP» — тема коммита начинается с `WIP`: числа такого прогона предварительные. "
          "«Отставание от main» — сколько коммитов `origin/main` прогон не видел.", ""]
    if gt and not gt_prev and snap.get("graphic_no_prev"):
        L += [f"graphic_tests: Δ {NA} — {snap['graphic_no_prev']}.", ""]
    warn = []
    gap = source_gap(snap)
    if gap is not None and gap > STALE_DAYS:
        warn.append(f"свежие прогоны WPT и graphic_tests разнесены на {gap} дн. — отчёт сводит "
                    "разные состояния кода, их цифры между собой не сопоставимы")
    if wpt_noise:
        warn.append(f"оба прогона WPT сняты на одном коммите `{wpt['commit']}` — их разница не от кода "
                    "и в просадки не идёт (раздел «Разброс прогонов»)")
    if wpt and wpt_prev and runner_diff(wpt, wpt_prev):
        warn.append("у прогонов WPT разные параметры раннера: " + ", ".join(runner_diff(wpt, wpt_prev))
                    + " — от них зависят CRASH и TIMEOUT")
    if warn:
        L += ["**Внимание:**", ""] + [f"- {w}." for w in warn] + [""]

    # Просадки
    L += ["## Просадки с прошлого отчёта", ""]
    if n_regs is None:
        L += [f"{NA} — нет предыдущего прогона для сравнения.", ""]
    elif n_regs == 0:
        L += ["Просадок нет." + (" Разница прогонов WPT одного коммита — в разделе «Разброс прогонов»."
                                 if wpt_noise and (wpt_regs or status_regs) else ""), ""]
    else:
        if not wpt_noise:
            L += wpt_drop_lines()
        for stem, old, new, d_new, _ in gt_regs or []:
            L.append(f"- graphic_tests `{stem}`: {old} → {new} (diff {d_new}%).")
        L.append("")

    if wpt_noise:
        L += ["## Разброс прогонов", ""]
        L += [f"Оба прогона WPT — коммит `{wpt['commit']}`: код один, разница — от окружения "
              "(параметры раннера, нагрузка машины, флаки). Это мера шума прогона, а не просадки.", ""]
        L += (wpt_drop_lines() or ["Заметной разницы нет."]) + [""]

    # WPT статусы
    L += ["## WPT: статусы тестов", ""]
    if wpt:
        rows = []
        for s in STATUS_ORDER:
            cur = wpt["statuses"][s]
            old = wpt_prev["statuses"][s] if wpt_prev else None
            rows.append((s, cur, NA if old is None else old, signed(diff(cur, old))))
        ids = wpt["totals"].get("ids")
        old_ids = wpt_prev["totals"].get("ids") if wpt_prev else None
        rows.append(("**всего id**", ids, NA if old_ids is None else old_ids, signed(diff(ids, old_ids))))
        L += md_table(["Статус", "Сейчас", "Было", "Δ"], rows) + [""]
        sub_prev = wpt_prev["subtest_rate"] if wpt_prev else None
        L += [f"Pass rate корпуса {pct(wpt['totals'].get('pass_rate'))} — балл (PASS = 1, testharness — доля "
              f"пройденных сабтестов) на все id корпуса; UNSUPPORTED (нет исполнителя: aamtest, print-reftest, "
              f"wdspec), SKIPPED и NOT-RUN дают 0. Сабтесты: {wpt['totals'].get('subtests_passed')}/"
              f"{wpt['totals'].get('subtests_total')} = {pct(wpt['subtest_rate'])} (Δ {pp(wpt['subtest_rate'], sub_prev)}). "
              "OK — testharness-файл отработал, его сабтесты считаются отдельно.", ""]
        if not wpt["status_sum_ok"]:
            L += ["**Внимание:** сумма статусов не равна числу id — агрегат неполон.", ""]
    else:
        L += [f"{NA} — в `docs/wpt/runs/` нет полного прогона корпуса.", ""]

    # Подсистемы
    L += ["## WPT по подсистемам", ""]
    if wpt:
        rows = []
        names = [n for n, _ in SUBSYSTEMS] + [OTHER_SUBSYSTEM]
        for name in names:
            cur = wpt["subsystems"].get(name)
            if not cur:
                continue
            old = wpt_prev["subsystems"].get(name) if wpt_prev else None
            rows.append((name, cur["ids"], cur["ran"], cur["harness_ok"],
                         f"{cur['subtests_passed']}/{cur['subtests_total']}", pct(cur["pass_rate"]),
                         pp(cur["pass_rate"], old and old["pass_rate"])))
        r, ro = wpt["rendering"], wpt_prev and wpt_prev["rendering"]
        rows.append((f"*{RENDERING}*", r["ids"], r["pass"] + r["fail"], r["pass"], "—",
                     pct(r["pass_rate"]), pp(r["pass_rate"], ro and ro["pass_rate"])))
        L += md_table(["Подсистема", "Тестов", "Прогнано", "OK/PASS", "Сабтесты", "Pass rate", "Δ"], rows) + [""]
        L += [f"*{RENDERING}* — срез поперёк категорий, в суммы строк выше уже входит: PASS из всех "
              "reftest/crashtest на их число («Прогнано» — сколько дали PASS или FAIL).", ""]
    else:
        L += [NA, ""]

    # Топ-5 багов
    L += ["## Топ-5 багов по числу заблокированных WPT-тестов", ""]
    if bugs and bugs["top5"]:
        rows = [(f"[{b['bug']}](../bugs/{b['bug']}-OPEN.md)", "/".join(b["components"]), b["blocked"],
                 b["metadata"], b["roadmap"], b["wpt_status"], ", ".join(b["tasks"]) or "—")
                for b in bugs["top5"]]
        L += md_table(["Баг", "Компонент", "Тестов", "metadata", "ROADMAP", "wpt-status", "Задачи"], rows) + [""]
        L += ["«Тестов» — объединение тестов из `tests/wpt/metadata/**/*.ini` (ожидание называет баг), "
              "WPT-путей в заметке незакрытой строки `ROADMAP.md` с этим багом в колонке `bugs` (каталог "
              f"шире {ROADMAP_DIR_CAP} тестов не считается) и детальных таблиц `docs/wpt-status.md`, плюс доля "
              "категории из категорийного индекса `docs/wpt-status.md` (её тесты не OK/PASS поровну между "
              "открытыми багами строки). Колонки справа — вклад каждого источника.", ""]
    else:
        L += [NA if not bugs else "Ни один открытый баг не связан с WPT-тестами.", ""]

    # Баги по компонентам
    L += ["## Открытые баги по подсистемам", ""]
    if bugs:
        prev_comp = prev_snap["bugs"]["by_component"] if prev_snap and prev_snap.get("bugs") else None
        rows = [(f"`{c}`", n, signed(diff(n, prev_comp.get(c, 0))) if prev_comp is not None else NA)
                for c, n in list(bugs["by_component"].items())[:20]]
        L += [f"Открыто {bugs['open']}, из них доработок (`ДОРАБОТКА → …`) {bugs['rework']}, "
              f"должников (`DEBTOR`) {bugs['debtor']}. Баг с несколькими компонентами считается в каждом.", ""]
        L += md_table(["Компонент", "Открыто", "Δ"], rows) + [""]
    else:
        L += [NA, ""]

    # graphic_tests
    L += ["## graphic_tests", ""]
    if gt:
        rows = []
        for s in GT_STATUS_ORDER:
            old = gt_prev["counts"].get(s, 0) if gt_prev else None
            rows.append((s, gt["counts"].get(s, 0), NA if old is None else old,
                         signed(diff(gt["counts"].get(s, 0), old))))
        L += [f"Pass rate {pct(gt['pass_rate'])} ({gt['counts']['PASS']}/{gt['total']}; DEBTOR — "
              "известный долг с храповиком, в PASS не входит), Δ "
              f"{pp(gt['pass_rate'], gt_prev and gt_prev['pass_rate'])}", ""]
        L += md_table(["Статус", "Сейчас", "Было", "Δ"], rows) + [""]
        if gt["summary_mismatch"]:
            L += [f"**Внимание:** `summary` прогона расходится с пересчётом по `tests` "
                  f"({', '.join(gt['summary_mismatch'])}).", ""]
        if gt_changes is None:
            L += [f"Сменившие статус: {NA} — нет предыдущего полного прогона.", ""]
        elif not gt_changes:
            L += ["Сменивших статус тестов нет.", ""]
        else:
            L += ["Сменившие статус:", ""]
            L += md_table(["Тест", "Было", "Стало", "diff %"],
                          [(f"`{s}`", o, n, NA if d is None else d) for s, o, n, d, _ in gt_changes]) + [""]
            reclass = sum(1 for _, o, n, _, _ in gt_changes if {o, n} == {"FAIL", "DEBTOR"})
            if reclass:
                L += [f"Из них FAIL ↔ DEBTOR: {reclass} — переклассификация храповика, не исправление и не "
                      "поломка: FAIL дают и выход за baseline должника, и улучшение ниже него (надо снизить "
                      "baseline). Строки FAIL и DEBTOR выше сдвигаются на это число, даже если отрисовка "
                      "не менялась.", ""]
        if gt_cap:
            names = ", ".join(f"`{s}` ({o or NA} → {n or NA})" for s, o, n in gt_cap[:5])
            more = f" и ещё {len(gt_cap) - 5}" if len(gt_cap) > 5 else ""
            L += [f"Сменился способ съёмки: {len(gt_cap)} — {names}{more}. Эти тесты не входят ни в "
                  "«Сменившие статус», ни в просадки: их сдвиг нельзя отличить от разницы способа съёмки.", ""]
    else:
        L += [f"{NA} — в `graphic_tests/results/` нет прогонов.", ""]

    # Категории
    L += ["## WPT по категориям", ""]
    if wpt:
        rows = []
        for cat, row in sorted(wpt["categories"].items(), key=lambda kv: (-(kv[1]["ids"] or 0), kv[0])):
            old = wpt_prev["categories"].get(cat) if wpt_prev else None
            rows.append((f"`{cat}`", row["ids"], row["harness_ok"],
                         signed(diff(row["harness_ok"], old and old["harness_ok"])),
                         pct(row["pass_rate"]), pp(row["pass_rate"], old and old["pass_rate"])))
        L += ["<details><summary>Все категории верхнего уровня (по числу тестов)</summary>", ""]
        L += md_table(["Категория", "Тестов", "OK/PASS", "Δ", "Pass rate", "Δ"], rows)
        L += ["", "</details>", ""]
    else:
        L += [NA, ""]
    return "\n".join(L), head


def build(root: Path) -> dict:
    runs = wpt_runs(root / "docs" / "wpt" / "runs")
    wpt = wpt_summary(*runs[-1]) if runs else None
    wpt_prev = wpt_summary(*runs[-2]) if len(runs) > 1 else None
    gruns = graphic_runs(root / "graphic_tests" / "results")
    cur_run, prev_run, why = graphic_pair(gruns)
    gt = graphic_summary(*cur_run, root=root) if cur_run else None
    gt_prev = graphic_summary(*prev_run, root=root) if prev_run else None
    snap = {"schema": 1, "wpt": wpt, "wpt_prev": wpt_prev, "graphic": gt, "graphic_prev": gt_prev,
            "graphic_no_prev": why, "bugs": bugs_summary(root, wpt)}
    if wpt and wpt_prev:
        snap["wpt_worst"] = wpt_worst(root / "docs" / "wpt" / "runs", runs[-1], runs[-2],
                                      wpt_regressions(wpt, wpt_prev)[:25])
    snap["stamp"] = snapshot_stamp(snap)
    # Отставание зависит от состояния git, а не от прогонов — в срез не пишется.
    snap["lag"] = {"wpt": commit_lag(root, wpt and wpt["commit"]),
                   "graphic": commit_lag(root, gt and gt["commit"])}
    return snap


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="только итоговая строка в stdout, без записи файлов")
    ap.add_argument("--root", type=Path, default=ROOT, help="корень репозитория (для тестов)")
    args = ap.parse_args(argv)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    root = args.root
    snap = build(root)
    runs_dir = root / "docs" / "quality" / "runs"
    payload = snapshot_payload(snap)
    # Срез хранит только свежие данные; предыдущие прогоны раннеров в нём не нужны.
    payload_core = {k: v for k, v in payload.items()
                    if k not in ("wpt_prev", "graphic_prev", "graphic_no_prev", "lag",
                                 "wpt_worst")}
    prev_snap, write = previous_snapshot(runs_dir, payload_core)
    text, head = render(snap, prev_snap)
    if args.check:
        print(head)
        return 0
    report = root / "docs" / "quality-report.md"
    report.parent.mkdir(parents=True, exist_ok=True)
    with open(report, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    if write:
        runs_dir.mkdir(parents=True, exist_ok=True)
        out = runs_dir / f"{snap['stamp']}.json"
        n = 1
        while out.exists():
            n += 1
            out = runs_dir / f"{snap['stamp']}-{n}.json"
        with open(out, "w", encoding="utf-8", newline="\n") as f:
            json.dump(payload_core, f, ensure_ascii=False, indent=1, sort_keys=True)
            f.write("\n")
        print(f"срез: {out.relative_to(root).as_posix()}")
    else:
        print("срез: без изменений — новый не записан")
    print(f"отчёт: {report.relative_to(root).as_posix()}")
    print(head)
    return 0


if __name__ == "__main__":
    sys.exit(main())
