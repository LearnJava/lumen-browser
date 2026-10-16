#!/usr/bin/env python3
"""Генератор таблицы CT-журналов `crates/network/src/tls/ct_logs_data.rs` (COMPAT-TRUST-CTLOGS).

Источники (скачать самому, скрипт сеть не трогает):

    curl -o google.json https://www.gstatic.com/ct/log_list/v3/log_list.json
    curl -o yandex.json https://browser-resources.s3.yandex.net/ctlog/ctlog.json
    curl -o apple.json  https://valid.apple.com/ct/log_list/current_log_list.json

Сверка перед записью (любое расхождение — отказ, файл не пишется):
  * `log_id` каждой записи == SHA-256(`key`) (RFC 6962 §3.2);
  * ключ журнала, который есть и в списке Google, и в списке Apple, совпадает
    (Apple — второй независимый источник; имена операторов у них пишутся
    по-разному, ключи — нет);
  * ни один журнал Яндекса не пересекается с Google по `log_id`.

Использование:
    python scripts/gen_ct_logs.py google.json yandex.json apple.json
"""

from __future__ import annotations

import base64
import hashlib
import json
import sys
from datetime import datetime
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
OUT = REPO_ROOT / "crates" / "network" / "src" / "tls" / "ct_logs_data.rs"

# Состояния, по которым SCT журнала принимается. `qualified` — журнал принят
# политикой, но ещё не принимает новых подач; уже выданные SCT он подтверждает так же.
ACCEPTED = {"usable", "qualified", "readonly", "retired"}


def iter_logs(doc):
    for op in doc["operators"]:
        for kind in ("logs", "tiled_logs"):
            for log in op.get(kind, []):
                yield op["name"], log


def epoch_ms(iso: str) -> int:
    return int(datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp() * 1000)


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def state_expr(state: dict) -> str | None:
    kind = next(iter(state))
    if kind not in ACCEPTED:
        return None
    if kind == "retired":
        return f"LogState::Retired {{ at_ms: {epoch_ms(state[kind]['timestamp'])} }}"
    if kind == "readonly":
        return "LogState::ReadOnly"
    return "LogState::Usable"


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__)
        return 2
    google, yandex, apple = (json.loads(Path(p).read_text(encoding="utf-8")) for p in sys.argv[1:4])

    entries = []  # (operator, description, state_expr, id_hex, spki_hex)
    seen = set()
    errors = []
    apple_keys = {l["log_id"]: l["key"] for _, l in iter_logs(apple)}
    google_ids = {l["log_id"] for _, l in iter_logs(google)}

    for source, doc in (("google", google), ("yandex", yandex)):
        for operator, log in iter_logs(doc):
            desc = log["description"]
            key = base64.b64decode(log["key"])
            log_id = base64.b64decode(log["log_id"])
            if hashlib.sha256(key).digest() != log_id:
                errors.append(f"{source}: log_id != sha256(key): {desc}")
            if source == "google" and log["log_id"] in apple_keys and apple_keys[log["log_id"]] != log["key"]:
                errors.append(f"google/apple: ключи расходятся: {desc}")
            if source == "yandex" and log["log_id"] in google_ids:
                errors.append(f"yandex/google: пересечение log_id: {desc}")
            if desc.lower().startswith("bogus"):
                continue  # заглушки Google «для библиотек», настоящего журнала за ними нет
            state = state_expr(log["state"])
            if state is None or log["log_id"] in seen:
                continue
            seen.add(log["log_id"])
            entries.append((operator, desc, state, log_id.hex(), key.hex()))

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1

    entries.sort(key=lambda e: (e[0], e[1]))
    lines = [
        "//! Таблица CT-журналов — СГЕНЕРИРОВАНО `scripts/gen_ct_logs.py`, руками не править.",
        "//!",
        f"//! Google log_list v3 от {google['log_list_timestamp']}, Яндекс ctlog.json от "
        f"{yandex['log_list_timestamp']}; ключи Google сверены со списком Apple.",
        f"//! Записей: {len(entries)}. Обновление — см. docstring скрипта и `ct_logs.rs`.",
        "",
        "use super::ct_logs::{LogState, RawLog};",
        "",
        "#[rustfmt::skip]",
        "pub(super) const RAW_LOGS: &[RawLog] = &[",
    ]
    for operator, desc, state, id_hex, spki_hex in entries:
        lines.append(
            f"    RawLog {{ operator: {rust_str(operator)}, name: {rust_str(desc)}, state: {state}, "
            f"id_hex: \"{id_hex}\", spki_hex: \"{spki_hex}\" }},"
        )
    lines += ["];", ""]
    OUT.write_text("\n".join(lines), encoding="utf-8", newline="\n")
    print(f"{len(entries)} журналов -> {OUT.relative_to(REPO_ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
