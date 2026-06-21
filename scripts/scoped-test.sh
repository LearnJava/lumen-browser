#!/usr/bin/env bash
# Запускает `cargo test` только для затронутых (относительно базовой ветки)
# крейтов + их транзитивных обратных зависимостей, а не для всего workspace.
#
# Зачем: финальный гейт (lumen-task-finish) уже гонит `cargo clippy --workspace
# --all-targets` — он КОМПИЛИРУЕТ весь workspace и ловит кросс-крейтовую поломку
# сборки. Поэтому полный `cargo test --workspace` после него избыточен: на 22
# крейта это ~110 отдельных линковок тест-бинарей (~30 мин). Замеры и обоснование
# — память project_build_test_perf_findings.
#
# Использование:  bash scripts/scoped-test.sh [base-ref]   (по умолчанию base = main)
#
# Ограничение: lumen-driver зависит почти от всего, поэтому почти любая правка
# втягивает его 64 тест-бинаря — основной потолок снимает только консолидация
# driver-тестов (отдельная задача), не этот скрипт.

export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
cd "$(git rev-parse --show-toplevel)" || exit 1

base="${1:-main}"

# имя пакета из каталога с Cargo.toml
pkg_name() { sed -n 's/^name *= *"\(.*\)".*/\1/p' "$1/Cargo.toml" | head -1; }

# 1. затронутые пакеты: коммиты ветки относительно base + рабочее дерево + индекс
changed=$( { git diff --name-only "$base"...HEAD 2>/dev/null
             git diff --name-only
             git diff --name-only --cached; } | sort -u )

pkgs=""
for f in $changed; do
  d=$(dirname "$f")
  while [ "$d" != "." ] && [ ! -f "$d/Cargo.toml" ]; do d=$(dirname "$d"); done
  [ -f "$d/Cargo.toml" ] || continue
  name=$(pkg_name "$d")
  [ -n "$name" ] && pkgs="$pkgs $name"
done
pkgs=$(printf '%s\n' $pkgs | sed '/^$/d' | sort -u)

if [ -z "$(printf '%s' "$pkgs" | tr -d '[:space:]')" ]; then
  echo "Изменённых крейтов нет (правки только в доках/конфигах) — тесты не нужны."
  exit 0
fi

# 2. транзитивное замыкание обратных зависимостей (кто зависит от затронутых)
all="$pkgs"
frontier="$pkgs"
while [ -n "$(printf '%s' "$frontier" | tr -d '[:space:]')" ]; do
  new=""
  for p in $frontier; do
    for m in $(grep -rlE "(^|[^A-Za-z0-9_-])$p([^A-Za-z0-9_-]|\$)" crates --include=Cargo.toml 2>/dev/null); do
      name=$(pkg_name "$(dirname "$m")")
      [ -n "$name" ] || continue
      case " $all " in
        *" $name "*) : ;;
        *) new="$new $name"; all="$all $name" ;;
      esac
    done
  done
  frontier="$new"
done
all=$(printf '%s\n' $all | sed '/^$/d' | sort -u)

args=""
for p in $all; do args="$args -p $p"; done

echo "Затронуто:  $(printf '%s ' $pkgs)"
echo "Тестирую (затронутые + обратные зависимости):"
printf '  %s\n' $all
echo "+ cargo test$args"
exec cargo test $args
