#!/usr/bin/env python3
"""Раннер сквозных сценариев совместимости (COMPAT-CORPUS-3, корпус S, метрика SPR-w).

Сценарий — это путь пользователя через сайт (вход до пароля, оплата до 3DS, видео,
почта, загрузка файла): шаги, оракул конца и вес. Сценарии лежат в
`docs/perf/scenarios/*.toml` (формат — docs/compat-scenarios.md). Раннер ведёт живое окно
Lumen через MCP (`perf_audit.LiveBrowser`), после шагов ждёт оракул и записывает исход:

  pass   оракул выполнился;
  fail   шаг не прошёл или оракул не выполнился за `oracle_wait_s`;
  crash  окно упало (сокет MCP закрыт, процесс завершён);
  hang   окно перестало отвечать (таймаут команды);
  skip   сценарий не запускался (ручной режим, нет учётной записи, передача).

SPR-w (отчёт docs/research/site-compat/report.md §7.8) — взвешенная доля пройденных
сценариев: вес 3 — вход, оплата, банк, госуслуги; 2 — видео, почта, документы, звонки;
1 — остальное. Крах и зависание — провал. Сценарии с плановой передачей (DRM, ГОСТ, ЭП,
звонки до этапа 3) и стенды самопроверки раннера в SPR-w не входят.

У каждого провала записан «распознанный экран» (`screen.class`): стена антибота,
капча, страница ошибки Lumen (`ERR_*`), баннер согласия, пустой экран, крах, зависание
или обычная страница без ожидаемого элемента, плюс кадр и начало видимого текста.

Результат: .tmp/compat-scenarios/<timestamp>/ (results.jsonl, summary.md, summary.json,
кадры провалов) — НЕ коммитится. `--resume DIR` продолжает прерванный прогон.

  python scripts/compat_scenarios.py --list
  python scripts/compat_scenarios.py --validate
  python scripts/compat_scenarios.py --nightly                    # режим nightly
  python scripts/compat_scenarios.py --only login-google,stand-   # подстроки id
  python scripts/compat_scenarios.py --self-test
"""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
import tomllib
from datetime import datetime
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

SCRIPTS_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPTS_DIR.parent
sys.path.insert(0, str(SCRIPTS_DIR))

import wall_signatures as ws  # noqa: E402

SCENARIO_DIR = REPO_ROOT / "docs" / "perf" / "scenarios"
STAND_DIR = REPO_ROOT / "samples" / "compat-scenarios"
OUT_ROOT = REPO_ROOT / ".tmp" / "compat-scenarios"

# Вес по категории (§7.8): 3 — вход, оплата, банк, госуслуги; 2 — видео, почта, документы, звонки.
CATEGORY_WEIGHT = {
    "login": 3, "bank": 3, "payment": 3, "gov": 3,
    "video": 2, "mail": 2, "docs": 2, "call": 2,
    "file": 1, "shop": 1, "news": 1, "search": 1, "social": 1, "maps": 1, "other": 1,
}
HANDOFFS = ("drm", "gost", "ep", "calls")
MODES = ("nightly", "manual")
STEP_KINDS = {
    "navigate": ("url",),
    "click": ("selector",),
    "click_text": ("text",),
    "type": ("selector", "text"),
    "wait": ("cond",),
    "sleep": ("seconds",),
    "eval": ("code",),
}
WAIT_CONDS = ("document_ready", "visible", "stable", "network_idle", "js_idle")
ORACLE_KINDS = ("visible", "url", "text", "not_text", "eval")
OUTCOMES = ("pass", "fail", "crash", "hang", "skip")

DEFAULT_TIMEOUT_S = 90
DEFAULT_ORACLE_WAIT_S = 10

# ── Загрузка и проверка сценариев ────────────────────────────────────────────


def effective_weight(sc: dict) -> int:
    return int(sc.get("weight") or CATEGORY_WEIGHT.get(sc.get("category", "other"), 1))


def counts_in_spr(sc: dict) -> bool:
    """Входит ли сценарий в SPR-w: не передача и не стенд самопроверки раннера."""
    return not sc.get("handoff") and not sc.get("stand")


def validate_scenario(sc: dict) -> list[str]:
    """Список нарушений формата одного сценария (пустой — формат верен)."""
    errs: list[str] = []
    sid = sc.get("id", "?")

    def bad(msg: str) -> None:
        errs.append(f"{sid}: {msg}")

    if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", str(sc.get("id", ""))):
        bad("id должен быть вида [a-z0-9-]+")
    if not sc.get("title"):
        bad("нет title")
    cat = sc.get("category")
    if cat not in CATEGORY_WEIGHT:
        bad(f"category {cat!r} не из {sorted(CATEGORY_WEIGHT)}")
    if sc.get("weight") is not None and sc["weight"] not in (1, 2, 3):
        bad("weight только 1, 2 или 3")
    if sc.get("mode", "nightly") not in MODES:
        bad(f"mode не из {MODES}")
    if sc.get("handoff") and sc["handoff"] not in HANDOFFS:
        bad(f"handoff не из {HANDOFFS}")
    if cat == "call" and sc.get("handoff") != "calls":
        bad("звонки до этапа 3 — только handoff = \"calls\" (вне SPR-w)")
    if sc.get("lang", "en") not in ("ru", "en"):
        bad("lang только ru или en")
    steps = sc.get("steps")
    if not isinstance(steps, list) or not steps:
        bad("нет steps")
        steps = []
    for i, st in enumerate(steps):
        kind = st.get("do")
        if kind not in STEP_KINDS:
            bad(f"шаг {i}: do {kind!r} не из {sorted(STEP_KINDS)}")
            continue
        for key in STEP_KINDS[kind]:
            if key not in st:
                bad(f"шаг {i} ({kind}): нет поля {key}")
        if kind == "wait" and st.get("cond") not in WAIT_CONDS:
            bad(f"шаг {i}: cond не из {WAIT_CONDS}")
        if kind == "wait" and st.get("cond") in ("visible", "stable") and not st.get("selector"):
            bad(f"шаг {i}: для {st['cond']} нужен selector")
    oracle = sc.get("oracle")
    if not isinstance(oracle, list) or not oracle:
        bad("нет oracle (оракул конца обязателен)")
        oracle = []
    for i, o in enumerate(oracle):
        if o.get("kind") not in ORACLE_KINDS:
            bad(f"oracle {i}: kind не из {ORACLE_KINDS}")
        elif not o.get("value"):
            bad(f"oracle {i}: нет value")
        elif o["kind"] in ("url", "text", "not_text"):
            try:
                re.compile(o["value"])
            except re.error as e:
                bad(f"oracle {i}: плохая регулярка: {e}")
    return errs


def load_scenarios(paths: list[Path] | None = None) -> tuple[list[dict], list[str]]:
    """Все сценарии из файлов `*.toml` и ошибки формата (в т.ч. повтор id)."""
    files = paths or sorted(SCENARIO_DIR.glob("*.toml"))
    scenarios: list[dict] = []
    errors: list[str] = []
    seen: dict[str, str] = {}
    for f in files:
        try:
            data = tomllib.loads(f.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as e:
            errors.append(f"{f.name}: {e}")
            continue
        for sc in data.get("scenario", []):
            sc["file"] = f.name
            errors.extend(validate_scenario(sc))
            sid = sc.get("id", "?")
            if sid in seen:
                errors.append(f"{sid}: повтор id ({seen[sid]} и {f.name})")
            seen[sid] = f.name
            scenarios.append(sc)
    return scenarios, errors


def select(scenarios: list[dict], only: list[str], nightly: bool, with_stands: bool) -> list[dict]:
    out = []
    for sc in scenarios:
        if only and not any(o in sc["id"] for o in only):
            continue
        if nightly and sc.get("mode", "nightly") != "nightly":
            continue
        if sc.get("stand") and not (with_stands or only):
            continue
        out.append(sc)
    return out


# ── Распознавание экрана провала ─────────────────────────────────────────────

CAPTCHA_RE = re.compile(
    r"captcha|recaptcha|hcaptcha|smartcaptcha|turnstile|я не робот|вы не робот|подтвердите,? что вы|"
    r"verify (that )?you are (a )?human|are you a robot|checking your browser", re.IGNORECASE)
CONSENT_RE = re.compile(
    r"accept (all )?cookies|принять (все )?(файлы )?cookie|мы используем (файлы )?cookie|"
    r"we use cookies|cookie (consent|preferences)|согласи(е|тесь) на (использование )?cookie", re.IGNORECASE)
LUMEN_ERR_RE = re.compile(r"\bERR_[A-Z0-9_]{3,}\b")
BLANK_TEXT_MAX = 20
EXCERPT_MAX = 240

SCREEN_CLASSES = ("crash", "hang", "lumen_error", "wall", "captcha", "consent", "blank", "content")


def classify_screen(state: dict | None, status: int | None = None, dead: bool = False,
                    hung: bool = False) -> dict:
    """Распознать экран, на котором встал сценарий.

    state — {href, title, text} (видимый текст страницы); status — HTTP-статус главного
    документа. Порядок важен: страница ошибки Lumen и жёсткая стена перекрывают капчу, капча — мягкую стену, баннер — последний.
    """
    state = state or {}
    text = (state.get("text") or "").strip()
    out = {"class": "content", "signals": [], "href": state.get("href", ""), "title": state.get("title", ""),
           "excerpt": re.sub(r"\s+", " ", text)[:EXCERPT_MAX]}
    if dead:
        out["class"] = "crash"
        return out
    if hung:
        out["class"] = "hang"
        return out
    err = LUMEN_ERR_RE.search(text)
    if err:
        out.update({"class": "lumen_error", "signals": [err.group(0)]})
        return out
    wall = ws.classify_wall(status, None, body=text, visible_text=text)

    def as_wall() -> dict:
        out.update({"class": "wall", "signals": [f"{wall['kind']}:{s}" for s in wall["signals"]] or [wall["kind"]]})
        if wall.get("vendor"):
            out["vendor"] = wall["vendor"]
        return out

    if wall["kind"] in ("hard", "refused"):
        return as_wall()
    if CAPTCHA_RE.search(text) or CAPTCHA_RE.search(state.get("title", "")):
        out["class"] = "captcha"
        return out
    if wall["kind"] == "soft":
        return as_wall()
    if len(text) < BLANK_TEXT_MAX:
        out["class"] = "blank"
        return out
    if CONSENT_RE.search(text):
        out["class"] = "consent"
    return out


# ── Метрика SPR-w ────────────────────────────────────────────────────────────


def spr_w(records: list[dict]) -> dict:
    """SPR-w по записям результатов. Сюда идут только исполненные сценарии,
    входящие в метрику; крах и зависание — провалы."""
    done = [r for r in records if r.get("counts") and r.get("outcome") in ("pass", "fail", "crash", "hang")]
    total = sum(r["weight"] for r in done)
    passed = sum(r["weight"] for r in done if r["outcome"] == "pass")
    failed = [r for r in done if r["outcome"] != "pass"]
    by_class: dict[str, int] = {}
    for r in failed:
        c = (r.get("screen") or {}).get("class", "none")
        by_class[c] = by_class.get(c, 0) + 1
    skipped = [r for r in records if r.get("outcome") == "skip"]
    return {
        "scenarios": len(done),
        "passed": sum(1 for r in done if r["outcome"] == "pass"),
        "failed": len(failed),
        "weight_total": total,
        "weight_passed": passed,
        "spr_w_pct": round(100.0 * passed / total, 1) if total else None,
        "base_reached": len(done) >= 40,
        "failures_by_screen": dict(sorted(by_class.items())),
        "failures_without_screen": sum(1 for r in failed if not (r.get("screen") or {}).get("class")),
        "flaky": sum(1 for r in done if r.get("flaky")),
        "skipped": len(skipped),
        "skipped_by_reason": _count(r.get("skip_reason", "?") for r in skipped),
        "crashes": sum(1 for r in done if r["outcome"] == "crash"),
        "hangs": sum(1 for r in done if r["outcome"] == "hang"),
    }


def _count(items) -> dict[str, int]:
    out: dict[str, int] = {}
    for it in items:
        out[it] = out.get(it, 0) + 1
    return dict(sorted(out.items()))


# ── Исполнение ───────────────────────────────────────────────────────────────


class DriverDead(Exception):
    """Окно упало (MCP-сокет закрыт)."""


class DriverHung(Exception):
    """Окно не ответило на команду вовремя."""


class StepFailed(Exception):
    """Шаг не удался (нет элемента, ошибка инструмента) — движок жив."""


def jsq(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def decode_eval(raw):
    """Ответ MCP eval — JSON-кодирование значения, иногда дважды."""
    for _ in range(3):
        if isinstance(raw, str):
            try:
                raw = json.loads(raw)
            except json.JSONDecodeError:
                break
        else:
            break
    return raw


STATE_JS = ("(() => JSON.stringify({href: location.href.slice(0, 300), title: document.title.slice(0, 160), "
            "text: ((document.body && (document.body.innerText || document.body.textContent)) || '').slice(0, 20000)}))()")

CLICK_TEXT_JS = """((re, scope) => {
  const rx = new RegExp(re, 'i');
  const els = Array.from(document.querySelectorAll(scope || 'button,a,[role=button],input[type=submit],input[type=button],label'));
  const el = els.find(e => rx.test((e.innerText || e.value || e.getAttribute('aria-label') || '').trim()) && e.getClientRects().length > 0);
  document.querySelectorAll('[data-lumen-scn]').forEach(n => n.removeAttribute('data-lumen-scn'));
  if (!el) return false;
  el.setAttribute('data-lumen-scn', '1');
  return true;
})(%s, %s)"""


class LiveDriver:
    """Живое окно Lumen через MCP (`perf_audit.LiveBrowser`)."""

    def __init__(self, browser, timeout_s: int) -> None:
        self.br = browser
        self.timeout_s = timeout_s

    def _tool(self, name: str, args: dict) -> dict:
        try:
            return self.br.mcp.tool(name, args)
        except RuntimeError as e:
            if "command timed out" in str(e):
                raise DriverHung(str(e)[:160]) from e
            raise StepFailed(str(e)[:200]) from e
        except (OSError, socket.timeout) as e:
            if isinstance(e, socket.timeout):
                raise DriverHung(str(e)[:160]) from e
            raise DriverDead(str(e)[:160]) from e

    def navigate(self, url: str) -> None:
        self._tool("navigate", {"url": url})

    def click(self, selector: str) -> None:
        self._tool("click", {"target": {"selector": selector}})

    def type_text(self, selector: str, text: str) -> None:
        self._tool("type", {"target": {"selector": selector}, "text": text})

    def wait(self, cond: str, selector: str | None, timeout_s: float) -> None:
        args = {"condition": cond, "timeout_ms": int(timeout_s * 1000)}
        if selector:
            args["selector"] = selector
        self._tool("wait", args)

    def eval(self, code: str):
        return decode_eval(self._tool("eval", {"code": code}).get("result"))

    def state(self) -> dict:
        try:
            val = self.eval(STATE_JS)
        except StepFailed:
            return {}
        return val if isinstance(val, dict) else {}

    def screenshot(self, path: Path) -> bool:
        import base64
        try:
            shot = self.br.mcp.resource("resource://screenshot")
        except (RuntimeError, OSError, socket.timeout):
            return False
        if shot and shot[0].get("data"):
            path.write_bytes(base64.b64decode(shot[0]["data"]))
            return True
        return False

    def alive(self) -> bool:
        return self.br.proc.poll() is None

    def restart(self) -> None:
        self.br.restart()

    def stderr_mark(self) -> int:
        return self.br.log_path.stat().st_size if self.br.log_path.exists() else 0

    def main_status(self, mark: int, url: str) -> int | None:
        import perf_audit as pa
        return pa.main_http_status(self.br.stderr_since(mark), url)


class ChromeDriver:
    """Эталон: Chrome for Testing через CDP, свой изолированный контекст на сценарий.

    Нужен, чтобы отличить провал Lumen от неверного оракула или отказа сайта эталону:
    сценарий, не проходящий и в Chrome, в SPR-w не считается (`ref_fail`).
    """

    def __init__(self, ref, timeout_s: int) -> None:
        import chrome_interaction_baseline as cib
        from bench_chromium import WS
        self.ref, self.timeout_s = ref, timeout_s
        self.ctx = ref.browser.call("Target.createBrowserContext")["browserContextId"]
        self.tid = ref.browser.call("Target.createTarget", {"url": "about:blank", "browserContextId": self.ctx})["targetId"]
        self.cdp = cib.Cdp(WS(f"ws://127.0.0.1:{ref.port}/devtools/page/{self.tid}"))
        for dom in ("Page", "Network", "Runtime"):
            self.cdp.call(f"{dom}.enable")
        self.cdp.call("Emulation.setDeviceMetricsOverride", {"width": 1600, "height": 900, "deviceScaleFactor": 1,
                                                             "mobile": False})
        if ref.headless:
            ua = self.cdp.eval("navigator.userAgent") or ""
            self.cdp.call("Network.setUserAgentOverride", {"userAgent": ua.replace("HeadlessChrome", "Chrome")})

    def close(self) -> None:
        for fn in (lambda: self.cdp.ws.sock.close(),
                   lambda: self.ref.browser.call("Target.closeTarget", {"targetId": self.tid}),
                   lambda: self.ref.browser.call("Target.disposeBrowserContext", {"browserContextId": self.ctx})):
            try:
                fn()
            except Exception:  # noqa: BLE001 — уборка best-effort
                pass

    def _call(self, method: str, params: dict | None = None, timeout: float = 30):
        try:
            return self.cdp.call(method, params, timeout)
        except TimeoutError as e:
            raise DriverHung(str(e)) from e
        except RuntimeError as e:
            raise StepFailed(str(e)[:200]) from e
        except (OSError, ConnectionError) as e:
            raise DriverDead(str(e)[:160]) from e

    def navigate(self, url: str) -> None:
        nav = self._call("Page.navigate", {"url": url}, timeout=self.timeout_s)
        if nav.get("errorText"):
            raise StepFailed(f"навигация: {nav['errorText']}")
        self.wait("document_ready", None, min(self.timeout_s, 30))

    def _center(self, selector: str) -> tuple[float, float]:
        pt = self.eval("(() => { const e = document.querySelector(%s); if (!e) return null; "
                       "e.scrollIntoView({block: 'center'}); const r = e.getBoundingClientRect(); "
                       "return r.width > 0 && r.height > 0 ? [r.left + r.width / 2, r.top + r.height / 2] : null; })()"
                       % jsq(selector))
        if not isinstance(pt, list):
            raise StepFailed(f"нет видимого элемента {selector}")
        return pt[0], pt[1]

    def click(self, selector: str) -> None:
        x, y = self._center(selector)
        for typ in ("mousePressed", "mouseReleased"):
            self._call("Input.dispatchMouseEvent", {"type": typ, "x": x, "y": y, "button": "left", "clickCount": 1})

    def type_text(self, selector: str, text: str) -> None:
        if self.eval("(() => { const e = document.querySelector(%s); if (!e) return false; e.focus(); "
                     "if (e.select) e.select(); return true; })()" % jsq(selector)) is not True:
            raise StepFailed(f"нет элемента {selector}")
        self._call("Input.insertText", {"text": text})

    def wait(self, cond: str, selector: str | None, timeout_s: float) -> None:
        if cond in ("network_idle", "js_idle", "stable"):
            time.sleep(1.0)
            return
        expr = ("document.readyState === 'complete'" if cond == "document_ready" else
                "(() => { const e = document.querySelector(%s); if (!e) return false; const r = e.getBoundingClientRect(); "
                "return r.width > 0 && r.height > 0; })()" % jsq(selector or "body"))
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            if self.eval(expr) is True:
                return
            time.sleep(0.3)
        raise StepFailed(f"wait {cond}: таймаут")

    def eval(self, code: str):
        r = self._call("Runtime.evaluate", {"expression": code, "returnByValue": True})
        return decode_eval(r.get("result", {}).get("value"))

    def state(self) -> dict:
        try:
            val = self.eval(STATE_JS)
        except StepFailed:
            return {}
        return val if isinstance(val, dict) else {}

    def screenshot(self, path: Path) -> bool:
        import base64
        try:
            path.write_bytes(base64.b64decode(self._call("Page.captureScreenshot", {"format": "png"})["data"]))
            return True
        except (StepFailed, DriverHung, DriverDead, OSError):
            return False

    def alive(self) -> bool:
        return self.ref.alive()

    def restart(self) -> None:
        self.ref.restart()

    def stderr_mark(self) -> int:
        return len(self.cdp.events)

    def main_status(self, mark: int, url: str) -> int | None:
        status = None
        for ev in self.cdp.events[mark:]:
            p = ev.get("params") or {}
            if ev["method"] == "Network.responseReceived" and p.get("type") == "Document":
                status = int((p.get("response") or {}).get("status") or 0) or None
        return status


def subst(value, ctx: dict):
    """Подстановка {stand}, {user}, {pass} в строковых полях шага."""
    if isinstance(value, str):
        for k, v in ctx.items():
            value = value.replace("{" + k + "}", v)
    return value


def run_step(drv, step: dict, ctx: dict, default_wait_s: float) -> None:
    kind = step["do"]
    if kind == "navigate":
        drv.navigate(subst(step["url"], ctx))
    elif kind == "click":
        drv.click(subst(step["selector"], ctx))
    elif kind == "click_text":
        found = drv.eval(CLICK_TEXT_JS % (jsq(step["text"]), jsq(step.get("scope", ""))))
        if found is not True:
            raise StepFailed(f"нет кликабельного элемента с текстом /{step['text']}/")
        drv.click('[data-lumen-scn="1"]')
    elif kind == "type":
        drv.type_text(subst(step["selector"], ctx), subst(step["text"], ctx))
    elif kind == "wait":
        drv.wait(step["cond"], step.get("selector"), float(step.get("timeout_s", default_wait_s)))
    elif kind == "sleep":
        time.sleep(float(step["seconds"]))
    elif kind == "eval":
        val = drv.eval(subst(step["code"], ctx))
        if step.get("expect", False) and not val:
            raise StepFailed(f"eval вернул {val!r}")


def check_oracle(drv, oracle: dict) -> tuple[bool, str]:
    """Одна проверка оракула: (выполнена, пояснение)."""
    kind, value = oracle["kind"], oracle["value"]
    if kind == "visible":
        ok = drv.eval("(() => { const e = document.querySelector(%s); if (!e) return false; "
                      "const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; })()" % jsq(value))
        return ok is True, f"visible {value}"
    if kind == "eval":
        return bool(drv.eval(value)), "eval"
    st = drv.state()
    if kind == "url":
        return bool(re.search(value, st.get("href", ""))), f"url ~ /{value}/ (сейчас {st.get('href', '')[:80]})"
    text = st.get("text", "")
    hit = bool(re.search(value, text, re.IGNORECASE))
    return (hit if kind == "text" else not hit), f"{kind} /{value}/"


def wait_oracle(drv, oracles: list[dict], within_s: float) -> tuple[bool, str]:
    """Опрашивать оракул до `within_s`: все проверки должны выполниться одновременно."""
    deadline = time.monotonic() + within_s
    last = ""
    while True:
        misses = []
        for o in oracles:
            ok, why = check_oracle(drv, o)
            if not ok:
                misses.append(why)
        if not misses:
            return True, ""
        last = "; ".join(misses)
        if time.monotonic() >= deadline:
            return False, last
        time.sleep(0.5)


def attempt(drv, sc: dict, ctx: dict, out_dir: Path, tag: str) -> dict:
    """Один прогон сценария: исход, провалившийся шаг/оракул, экран."""
    rec = {"outcome": "pass", "failed_step": None, "failed_oracle": None}
    default_wait = min(float(sc.get("timeout_s", DEFAULT_TIMEOUT_S)), 60.0)
    mark = drv.stderr_mark()
    last_url = ""
    t0 = time.monotonic()
    dead = hung = False
    try:
        for i, step in enumerate(sc["steps"]):
            if step["do"] == "navigate":
                last_url = subst(step["url"], ctx)
            try:
                run_step(drv, step, ctx, default_wait)
            except StepFailed as e:
                if step.get("optional"):
                    continue
                rec.update(outcome="fail", failed_step={"index": i, "do": step["do"], "error": str(e)})
                break
            if time.monotonic() - t0 > float(sc.get("timeout_s", DEFAULT_TIMEOUT_S)):
                rec.update(outcome="fail", failed_step={"index": i, "do": step["do"], "error": "время сценария вышло"})
                break
        if rec["outcome"] == "pass":
            ok, why = wait_oracle(drv, sc["oracle"], float(sc.get("oracle_wait_s", DEFAULT_ORACLE_WAIT_S)))
            if not ok:
                rec.update(outcome="fail", failed_oracle=why)
    except DriverDead as e:
        dead = True
        rec.update(outcome="crash", failed_step={"error": str(e)})
    except DriverHung as e:
        hung = True
        rec.update(outcome="hang", failed_step={"error": str(e)})
    if rec["outcome"] == "pass" and not drv.alive():
        dead = True
        rec["outcome"] = "crash"
    rec["seconds"] = round(time.monotonic() - t0, 1)
    if rec["outcome"] != "pass":
        state = {} if (dead or hung) else drv.state()
        status = None if (dead or hung or not last_url) else drv.main_status(mark, last_url)
        rec["screen"] = classify_screen(state, status=status, dead=dead, hung=hung)
        rec["http_status"] = status
        if not (dead or hung):
            png = out_dir / f"{sc['id']}{tag}.png"
            if drv.screenshot(png):
                rec["screen"]["png"] = png.name
        if dead or hung or not drv.alive():
            drv.restart()
            rec["restarted"] = True
    return rec


def skip_reason(sc: dict, env: dict) -> str | None:
    if sc.get("handoff"):
        return f"handoff:{sc['handoff']}"
    if sc.get("account"):
        base = sc["account"]
        if not (env.get(f"{base}_USER") and env.get(f"{base}_PASS")):
            return "no_account"
    return None


def run_scenario(drv, sc: dict, ctx: dict, out_dir: Path, rerun: bool) -> dict:
    rec = {"id": sc["id"], "title": sc["title"], "category": sc["category"], "weight": effective_weight(sc),
           "lang": sc.get("lang", "en"), "counts": counts_in_spr(sc), "mode": sc.get("mode", "nightly"),
           "stand": bool(sc.get("stand")), "handoff": sc.get("handoff", "")}
    why = skip_reason(sc, os.environ)
    if why:
        rec.update(outcome="skip", skip_reason=why)
        return rec
    ctx = dict(ctx)
    if sc.get("account"):
        ctx["user"] = os.environ[f"{sc['account']}_USER"]
        ctx["pass"] = os.environ[f"{sc['account']}_PASS"]
    first = attempt(drv, sc, ctx, out_dir, "")
    rec.update(first)
    if first["outcome"] != "pass" and rerun:
        second = attempt(drv, sc, ctx, out_dir, ".retry")
        rec["retry"] = {k: second.get(k) for k in ("outcome", "failed_step", "failed_oracle", "seconds")}
        if second["outcome"] == "pass":
            # одиночный провал, повтор прошёл: сценарий пройден, но нестабилен
            rec.update(outcome="pass", flaky=True)
        elif second["outcome"] != first["outcome"]:
            rec["flaky"] = True
    return rec


# ── Стенд, вывод ─────────────────────────────────────────────────────────────


class _QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *a) -> None:  # noqa: D401
        pass


def start_stand(root: Path) -> tuple[ThreadingHTTPServer, str]:
    """Локальный HTTP-сервер для стендов самопроверки; возвращает (сервер, база URL)."""
    srv = ThreadingHTTPServer(("127.0.0.1", 0), partial(_QuietHandler, directory=str(root)))
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{srv.server_address[1]}"


def summary_md(records: list[dict], meta: dict) -> str:
    s = spr_w(records)
    lines = ["# Сквозные сценарии (COMPAT-CORPUS-3)", "",
             f"Коммит `{meta.get('commit', '?')}`, `{meta.get('exe', '?')}`, {meta.get('when', '')}", "",
             f"**SPR-w: {s['spr_w_pct']}%** — вес {s['weight_passed']} из {s['weight_total']}, "
             f"сценариев {s['passed']}/{s['scenarios']}"
             + ("" if s["base_reached"] else f" (база требует ≥40, исполнено {s['scenarios']})"), "",
             f"Крахи {s['crashes']}, зависания {s['hangs']}, нестабильные {s['flaky']}, пропущено {s['skipped']} "
             f"{s['skipped_by_reason']}.", "",
             f"Провалы по распознанному экрану: {s['failures_by_screen'] or 'нет'}; "
             f"без экрана: {s['failures_without_screen']}.", ""]
    fails = [r for r in records if r.get("outcome") in ("fail", "crash", "hang")]
    if fails:
        lines += ["## Провалы", "", "| id | вес | исход | экран | где встал |", "|---|---|---|---|---|"]
        for r in fails:
            scr = r.get("screen") or {}
            where = (r.get("failed_oracle")
                     or ((r.get("failed_step") or {}).get("error") or "")
                     or "")[:90].replace("|", "/")
            lines.append(f"| {r['id']} | {r['weight']} | {r['outcome']} | {scr.get('class', '?')}"
                         f"{' ' + ','.join(scr.get('signals', [])) if scr.get('signals') else ''} | {where} |")
        lines.append("")
    by_cat: dict[str, list[int]] = {}
    for r in records:
        if r.get("counts") and r.get("outcome") in ("pass", "fail", "crash", "hang"):
            by_cat.setdefault(r["category"], [0, 0])
            by_cat[r["category"]][1] += 1
            by_cat[r["category"]][0] += r["outcome"] == "pass"
    if by_cat:
        lines += ["## По категориям", "", "| категория | пройдено |", "|---|---|"]
        lines += [f"| {c} | {p}/{n} |" for c, (p, n) in sorted(by_cat.items())]
        lines.append("")
    return "\n".join(lines)


def read_results(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(ln) for ln in path.read_text(encoding="utf-8").splitlines() if ln.strip()]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--list", action="store_true", help="показать сценарии и выйти")
    ap.add_argument("--validate", action="store_true", help="проверить формат файлов и выйти")
    ap.add_argument("--self-test", action="store_true", help="прогнать тесты scripts/test_compat_scenarios.py")
    ap.add_argument("--only", default="", help="подстроки id через запятую")
    ap.add_argument("--nightly", action="store_true", help="только mode = nightly")
    ap.add_argument("--with-stands", action="store_true", help="включить стенды самопроверки раннера")
    ap.add_argument("--file", action="append", type=Path, help="файл сценариев (по умолчанию docs/perf/scenarios/*.toml)")
    ap.add_argument("--exe", help="путь к lumen.exe")
    ap.add_argument("--out", type=Path)
    ap.add_argument("--resume", type=Path)
    ap.add_argument("--no-rerun", action="store_true", help="не повторять провалившийся сценарий")
    ap.add_argument("--ref", action="store_true",
                    help="сначала пройти сценарий в Chrome (эталон); не прошёл и там — skip ref_fail, в SPR-w не идёт")
    ap.add_argument("--chrome-only", action="store_true",
                    help="калибровка оракулов: прогнать только в Chrome, без окна Lumen (результат — как у Lumen, с пометкой chrome_only)")
    ap.add_argument("--chrome", help="путь к chrome.exe (по умолчанию Chrome for Testing из ms-playwright)")
    ap.add_argument("--headless", action="store_true", help="Chrome без окна")
    ap.add_argument("--env", action="append", default=[], metavar="KEY=VAL", help="доп. окружение для lumen")
    args = ap.parse_args()

    if args.self_test:
        return subprocess.call([sys.executable, str(SCRIPTS_DIR / "test_compat_scenarios.py")])

    scenarios, errors = load_scenarios(args.file)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        if args.validate or not args.list:
            return 1
    only = [o for o in args.only.split(",") if o]
    chosen = select(scenarios, only, args.nightly, args.with_stands)
    if args.validate:
        print(f"[scenarios] {len(scenarios)} сценариев, формат верен")
        return 0
    if args.list:
        for sc in chosen:
            flags = ",".join(x for x in (sc.get("mode", "nightly"), sc.get("handoff", ""), "stand" if sc.get("stand") else "",
                                         "account" if sc.get("account") else "") if x)
            print(f"{sc['id']:40} w{effective_weight(sc)} {sc['category']:8} {sc.get('lang', 'en')} [{flags}] {sc['title']}")
        spr = [s for s in chosen if counts_in_spr(s)]
        print(f"[scenarios] выбрано {len(chosen)}, в SPR-w {len(spr)}, nightly {sum(1 for s in spr if s.get('mode', 'nightly') == 'nightly')}")
        return 0
    if not chosen:
        print("Нет сценариев под фильтр", file=sys.stderr)
        return 1

    import perf_audit as pa
    for kv in args.env:
        k, _, v = kv.partition("=")
        pa.EXTRA_ENV[k] = v
    out_dir = (args.resume or args.out or OUT_ROOT / datetime.now().strftime("%Y%m%d-%H%M%S")).resolve()
    (out_dir / "chrome").mkdir(parents=True, exist_ok=True)
    results_path = out_dir / "results.jsonl"
    records = read_results(results_path)
    done = {r["id"] for r in records}
    exe = pa.find_exe(args.exe) if not args.chrome_only else Path("chrome-only")
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                            cwd=REPO_ROOT).stdout.strip()
    meta = {"commit": commit, "exe": str(exe), "when": datetime.now().strftime("%Y-%m-%d %H:%M")}
    print(f"[scenarios] {len(chosen)} сценариев, готово {len(done)}, вывод {out_dir}", flush=True)

    stand_srv, stand_url = (None, "")
    if any(s.get("stand") for s in chosen):
        stand_srv, stand_url = start_stand(STAND_DIR)
    ctx = {"stand": stand_url}
    chrome = None
    if args.ref or args.chrome_only:
        import chrome_interaction_baseline as cib
        from compat_crawl import ChromeRef
        binary = args.chrome or cib.default_binary()
        if not binary:
            print("chrome.exe не найден: укажите --chrome или поставьте Chrome for Testing (ms-playwright)", file=sys.stderr)
            return 1
        meta["chrome"] = cib.chromium_version(binary)
        chrome = ChromeRef(binary, args.headless, out_dir)
    br = drv = None
    if not args.chrome_only:
        br = pa.LiveBrowser(exe, out_dir, DEFAULT_TIMEOUT_S)
        drv = LiveDriver(br, DEFAULT_TIMEOUT_S)
    try:
        for i, sc in enumerate(chosen, 1):
            if sc["id"] in done:
                continue
            ref_rec = None
            if args.chrome_only:
                cdrv = ChromeDriver(chrome, DEFAULT_TIMEOUT_S)
                try:
                    rec = run_scenario(cdrv, sc, ctx, out_dir, rerun=not args.no_rerun)
                finally:
                    cdrv.close()
                rec["chrome_only"] = True
                records.append(rec)
                with results_path.open("a", encoding="utf-8") as f:
                    f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                scr = (rec.get("screen") or {}).get("class", "")
                print(f"[{i}/{len(chosen)}] {sc['id']}: chrome {rec['outcome']}{' (' + scr + ')' if scr else ''}"
                      f"{' ' + str(rec.get('failed_oracle') or rec.get('failed_step') or '')[:110] if rec['outcome'] != 'pass' else ''}",
                      flush=True)
                continue
            if chrome is not None and not skip_reason(sc, os.environ):
                cdrv = ChromeDriver(chrome, DEFAULT_TIMEOUT_S)
                try:
                    ref_rec = run_scenario(cdrv, sc, ctx, out_dir / "chrome", rerun=True)
                finally:
                    cdrv.close()
            rec = run_scenario(drv, sc, ctx, out_dir, rerun=not args.no_rerun)
            if ref_rec is not None:
                rec["ref_outcome"] = ref_rec["outcome"]
                if ref_rec["outcome"] != "pass":
                    rec.update(lumen_outcome=rec["outcome"], outcome="skip", skip_reason="ref_fail",
                               ref_screen=ref_rec.get("screen"), ref_failed=ref_rec.get("failed_oracle")
                               or ref_rec.get("failed_step"))
            records.append(rec)
            with results_path.open("a", encoding="utf-8") as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
            scr = (rec.get("screen") or {}).get("class", "")
            print(f"[{i}/{len(chosen)}] {sc['id']}: {rec['outcome']}{' (' + scr + ')' if scr else ''}"
                  f"{' flaky' if rec.get('flaky') else ''}"
                  f"{' ref=' + rec['ref_outcome'] if rec.get('ref_outcome') else ''}"
                  f"{' lumen=' + rec['lumen_outcome'] if rec.get('lumen_outcome') else ''}", flush=True)
    finally:
        if br is not None:
            br.close()
        if chrome is not None:
            chrome.close()
        if stand_srv:
            stand_srv.shutdown()
    (out_dir / "summary.md").write_text(summary_md(records, meta), encoding="utf-8")
    s = spr_w(records)
    (out_dir / "summary.json").write_text(json.dumps(s, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"[scenarios] SPR-w {s['spr_w_pct']}% · {s['passed']}/{s['scenarios']} · провалов {s['failed']} · {out_dir / 'summary.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
