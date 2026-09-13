#!/usr/bin/env bash
# Оборачивает тяжёлую сборку (cargo build/test всего workspace или крупного
# крейта) курьезным (не жёстким) локом на файл, чтобы не совмещать её по
# времени с активным прогоном WPT (BUG-1029 §3): браузеры под тестом держат
# ~1 ГБ RSS каждый, сборка добавляет ~0.5-1 ГБ на джоб линкера/rustc — вместе
# на 7.6 ГБ машине это и увело её в OOM 2026-09-07 дважды.
#
# Опционально: обычный `cargo build -p <крейт> --profile dev-release` внутри
# рабочего слота использовать напрямую можно как раньше — этот скрипт нужен,
# когда сборка заведомо тяжёлая (--workspace, релизный профиль, полный тест-сьют)
# и в это время мог бы идти чужой WPT-прогон.
#
# Использование: bash scripts/cargo-heavy.sh build --workspace --profile dev-release
#                bash scripts/cargo-heavy.sh test --workspace
#
# Лок — обычный OS advisory lock (tests/wpt/.heavy.lock), освобождается сам
# при любом завершении процесса, включая kill -9 — подробности и почему это
# не жёсткий гейт (ожидание ограничено таймаутом с обеих сторон) — в
# tests/wpt/heavy_lock.py.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <cargo-subcommand> [args...]" >&2
  exit 2
fi

PYTHON="${PYTHON:-python}"
exec "$PYTHON" tests/wpt/heavy_lock.py run --owner "cargo $*" -- cargo "$@"
