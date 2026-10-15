#!/usr/bin/env python3
"""Робот доверия T: TLS-цепочка топ-200 RU изо дня в день (COMPAT-CORPUS-5, дорожка COMPAT).

Прибор — `lumen --dump-tls @список` (crates/network/src/tls/probe.rs): по хосту одна строка
JSON — issuer листа, цепочка, чем она заякорена (webpki / os / nuc / unknown), доверие
`LumenVerifier`, число SCT (по id логов), вердикт отзыва. Робот гоняет прибор по списку,
кладёт строки в базу по дням и сравнивает с предыдущим днём: смена CA, переезд на НУЦ
Минцифры, потеря доверия, пропажа SCT, отзыв.

  python scripts/tls_trust_robot.py list                  # пересобрать docs/perf/tls-top200-ru.txt
  python scripts/tls_trust_robot.py run                   # прогон + запись базы + дифф с прошлым днём
  python scripts/tls_trust_robot.py diff A.jsonl B.jsonl  # дифф двух снимков

База — docs/perf/tls-trust/YYYY-MM-DD.jsonl (по строке на хост, отсортирована по хосту; отчёт
диффа рядом — YYYY-MM-DD.md). Прогон из-за границы покажет «Unknown»-цепочки и отказы там, где
из РФ их нет (и наоборот): точку прогона указывать в `--point`, она пишется в шапку отчёта.
"""

from __future__ import annotations

import argparse
import collections
import datetime
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LIST_PATH = os.path.join(ROOT, "docs", "perf", "tls-top200-ru.txt")
CORPUS_TSV = os.path.join(ROOT, "docs", "perf", "corpus-c.tsv")
DB_DIR = os.path.join(ROOT, "docs", "perf", "tls-trust")
# Меньше стольких разных логов CT у листа — подозрительно (Chrome требует 2–3).
MIN_SCT_LOGS = 2

# Виды событий диффа; порядок задаёт порядок в отчёте (самые важные первыми).
EVENT_ORDER = [
    "lost_trust", "revoked", "root_kind_changed", "ca_changed", "sct_dropped",
    "handshake_failed", "gained_trust", "handshake_recovered", "host_added", "host_removed",
]


def read_snapshot(path: str) -> dict[str, dict]:
    """host -> строка прибора. Повтор хоста — побеждает последний."""
    out: dict[str, dict] = {}
    with open(path, encoding="utf-8", newline="") as f:
        for line in f:
            line = line.strip()
            if line:
                row = json.loads(line)
                out[row["host"]] = row
    return out


def write_snapshot(path: str, rows: list[dict]) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        for row in sorted(rows, key=lambda r: r["host"]):
            f.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")


def issuer_of(row: dict) -> str:
    return row.get("issuer_org") or row.get("issuer_cn") or ""


def diff_snapshots(old: dict[str, dict], new: dict[str, dict]) -> list[dict]:
    """События между двумя снимками. Перевыпуск листа (leaf_sha256) сам по себе не событие:
    это норма раз в 90 дней; событие — когда меняется тот, кто выпустил."""
    events: list[dict] = []

    def ev(kind: str, host: str, detail: str) -> None:
        events.append({"kind": kind, "host": host, "detail": detail})

    for host in sorted(new.keys() - old.keys()):
        ev("host_added", host, "нет в прошлом снимке")
    for host in sorted(old.keys() - new.keys()):
        ev("host_removed", host, "нет в новом снимке")
    for host in sorted(old.keys() & new.keys()):
        a, b = old[host], new[host]
        if not b.get("handshake_ok") and a.get("handshake_ok"):
            ev("handshake_failed", host, b.get("error", "") or "рукопожатие не завершилось")
            continue
        if b.get("handshake_ok") and not a.get("handshake_ok"):
            ev("handshake_recovered", host, "рукопожатие снова проходит")
        if not (a.get("seen_chain") and b.get("seen_chain")):
            continue
        if a.get("trusted") and not b.get("trusted"):
            ev("lost_trust", host, b.get("trust_error", "") or "цепочка не доверена")
        elif b.get("trusted") and not a.get("trusted"):
            ev("gained_trust", host, f"было: {a.get('trust_error', '')}")
        if b.get("revocation") == "revoked" and a.get("revocation") != "revoked":
            ev("revoked", host, "OCSP-ответ: отозван")
        if a.get("root_kind") != b.get("root_kind"):
            tail = " (переезд на НУЦ Минцифры)" if b.get("root_kind") == "nuc" else ""
            ev("root_kind_changed", host, f"{a.get('root_kind')} → {b.get('root_kind')}{tail}")
        if issuer_of(a) != issuer_of(b) or a.get("anchor_name") != b.get("anchor_name"):
            ev("ca_changed", host, f"{issuer_of(a)} / {a.get('anchor_name')} → "
                                    f"{issuer_of(b)} / {b.get('anchor_name')}")
        sa, sb = a.get("sct_logs", 0), b.get("sct_logs", 0)
        if sb < sa and sb < MIN_SCT_LOGS:
            ev("sct_dropped", host, f"SCT от разных логов: {sa} → {sb}")
    rank = {k: i for i, k in enumerate(EVENT_ORDER)}
    events.sort(key=lambda e: (rank.get(e["kind"], len(rank)), e["host"]))
    return events


def summarize(rows: dict[str, dict]) -> dict:
    ok = [r for r in rows.values() if r.get("handshake_ok")]
    return {
        "hosts": len(rows),
        "handshake_ok": len(ok),
        "failed": sorted(h for h, r in rows.items() if not r.get("handshake_ok")),
        "trusted": sum(1 for r in ok if r.get("trusted")),
        "untrusted": sorted(h for h, r in rows.items() if r.get("handshake_ok") and not r.get("trusted")),
        "root_kind": dict(collections.Counter(r.get("root_kind", "") for r in ok)),
        "issuer_org": collections.Counter(issuer_of(r) for r in ok).most_common(12),
        "few_sct": sorted(h for h, r in rows.items()
                          if r.get("handshake_ok") and r.get("sct_logs", 0) < MIN_SCT_LOGS),
        "revoked": sorted(h for h, r in rows.items() if r.get("revocation") == "revoked"),
        "staple": sum(1 for r in ok if r.get("revocation") in ("good", "revoked")),
    }


def render_report(date: str, point: str, new: dict[str, dict], prev_name: str | None,
                  events: list[dict], elapsed_s: float | None) -> str:
    s = summarize(new)
    lines = [f"# Робот доверия T — {date}", ""]
    lines.append(f"Точка прогона: {point or 'не указана'}. Хостов: {s['hosts']}, "
                 f"рукопожатий завершено: {s['handshake_ok']}, доверено: {s['trusted']}."
                 + (f" Время прогона: {elapsed_s:.0f} с." if elapsed_s is not None else ""))
    lines.append("")
    lines.append("Якорь цепочки: " + ", ".join(f"{k or '?'} — {v}" for k, v in sorted(s["root_kind"].items())))
    lines.append("")
    lines.append("Выпускающие (орг. листа): " + "; ".join(f"{k or '?'} — {v}" for k, v in s["issuer_org"]))
    lines.append("")
    lines.append(f"Меньше {MIN_SCT_LOGS} логов CT: {len(s['few_sct'])}. Скрепка OCSP у {s['staple']} из "
                 f"{s['handshake_ok']}. Отозвано: {len(s['revoked'])}.")
    if s["failed"]:
        lines += ["", "Не ответили: " + ", ".join(s["failed"])]
    if s["untrusted"]:
        lines += ["", "Недоверенные цепочки: " + ", ".join(s["untrusted"])]
    lines += ["", f"## Дифф с {prev_name}" if prev_name else "## Дифф", ""]
    if prev_name is None:
        lines.append("Прошлого снимка нет — это базовая линия.")
    elif not events:
        lines.append("Изменений нет.")
    else:
        by_kind = collections.Counter(e["kind"] for e in events)
        lines.append(", ".join(f"{k}: {by_kind[k]}" for k in EVENT_ORDER if k in by_kind))
        lines.append("")
        lines += [f"- `{e['kind']}` {e['host']}: {e['detail']}" for e in events]
    return "\n".join(lines) + "\n"


def previous_snapshot(date: str, db_dir: str) -> str | None:
    if not os.path.isdir(db_dir):
        return None
    older = sorted(n for n in os.listdir(db_dir) if n.endswith(".jsonl") and n[:-6] < date)
    return older[-1] if older else None


def run_probe(lumen: str, list_path: str, timeout_s: int) -> tuple[list[dict], float]:
    t0 = time.time()
    proc = subprocess.run([lumen, "--dump-tls", f"@{list_path}"], capture_output=True,
                          timeout=timeout_s)
    rows = []
    for line in proc.stdout.decode("utf-8", "replace").splitlines():
        line = line.strip()
        if line.startswith("{"):
            rows.append(json.loads(line))
    if proc.returncode != 0 and not rows:
        raise SystemExit(f"lumen --dump-tls: код {proc.returncode}: "
                         f"{proc.stderr.decode('utf-8', 'replace')[:300]}")
    return rows, time.time() - t0


def default_lumen() -> str:
    exe = "lumen.exe" if os.name == "nt" else "lumen"
    for base in (os.environ.get("CARGO_TARGET_DIR"), os.path.join(ROOT, "target")):
        if base:
            for prof in ("release", "debug"):
                p = os.path.join(base, prof, exe)
                if os.path.exists(p):
                    return p
    return exe


def cmd_list(_: argparse.Namespace) -> int:
    """Хосты сегмента ru корпуса C (госсайты, банки, РЖД, маркетплейсы входят по категориям)."""
    hosts: list[str] = []
    with open(CORPUS_TSV, encoding="utf-8", newline="") as f:
        for line in f:
            if line.startswith("#"):
                continue
            cols = line.rstrip("\r\n").split("\t")
            if len(cols) > 8 and cols[1] == "ru":
                host = cols[8].split("//", 1)[-1].split("/", 1)[0]
                if host not in hosts:
                    hosts.append(host)
    with open(LIST_PATH, "w", encoding="utf-8", newline="\n") as f:
        f.write("# Хосты для робота доверия T (scripts/tls_trust_robot.py): сегмент ru корпуса C.\n")
        f.write("# Сгенерировано `python scripts/tls_trust_robot.py list` из docs/perf/corpus-c.tsv — руками не править.\n")
        f.write("# Формат: один хост (или host:порт) в строке; `#` — комментарий.\n\n")
        for h in hosts:
            f.write(h + "\n")
    print(f"{len(hosts)} хостов -> {os.path.relpath(LIST_PATH, ROOT)}")
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    date = args.date or datetime.date.today().isoformat()
    rows, elapsed = run_probe(args.lumen or default_lumen(), args.list, args.timeout)
    snap = os.path.join(args.db, f"{date}.jsonl")
    prev_name = previous_snapshot(date, args.db)
    write_snapshot(snap, rows)
    new = {r["host"]: r for r in rows}
    events: list[dict] = []
    if prev_name:
        events = diff_snapshots(read_snapshot(os.path.join(args.db, prev_name)), new)
    report = render_report(date, args.point, new, prev_name, events, elapsed)
    with open(os.path.join(args.db, f"{date}.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write(report)
    print(report)
    return 2 if (events and args.fail_on_change) else 0


def cmd_diff(args: argparse.Namespace) -> int:
    events = diff_snapshots(read_snapshot(args.old), read_snapshot(args.new))
    for e in events:
        print(f"{e['kind']}\t{e['host']}\t{e['detail']}")
    print(f"событий: {len(events)}")
    return 2 if (events and args.fail_on_change) else 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list", help="пересобрать docs/perf/tls-top200-ru.txt").set_defaults(fn=cmd_list)
    p = sub.add_parser("run", help="прогон, запись базы, дифф с прошлым днём")
    p.add_argument("--lumen", help="путь к бинарю lumen (по умолчанию target/{release,debug})")
    p.add_argument("--list", default=LIST_PATH)
    p.add_argument("--db", default=DB_DIR)
    p.add_argument("--date", help="дата снимка YYYY-MM-DD (по умолчанию сегодня)")
    p.add_argument("--point", default="", help="точка прогона: ru / abroad / VPN — в шапку отчёта")
    p.add_argument("--timeout", type=int, default=900, help="потолок на весь прогон, с")
    p.add_argument("--fail-on-change", action="store_true", help="код 2, если дифф непуст")
    p.set_defaults(fn=cmd_run)
    d = sub.add_parser("diff", help="дифф двух снимков")
    d.add_argument("old")
    d.add_argument("new")
    d.add_argument("--fail-on-change", action="store_true")
    d.set_defaults(fn=cmd_diff)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
