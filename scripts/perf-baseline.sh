#!/usr/bin/env bash
# Baseline-бинарь main для A/B перф-замеров, с кэшом по SHA.
#
# Зачем: сравнение «ветка против main» требует собранного main. В сессии
# 2026-07-27 под это поднимался ОТДЕЛЬНЫЙ свежий worktree и собирался с нуля —
# 22 минуты, и так на каждый замер. Здесь main собирается в постоянном слоте
# `perf-base` (тёплый target/, пересборка инкрементальная), а готовый бинарь
# складывается в кэш по SHA: повторный запрос того же SHA стоит ноль.
#
# Использование:
#   bash scripts/perf-baseline.sh [--tests] [ref]
#
#   --tests  собрать тест-бинарь lumen-shell (`cargo test --no-run`) вместо
#            lumen.exe — им гоняются перф-гейты вроде CC12_HOVER/CC12_KEY
#   ref      что считать базой, по умолчанию main
#
# Последняя строка вывода — абсолютный путь бинаря (для `BASE="$(... | tail -1)"`).
#
# Почему не переиспользуем уже собранный бинарь из главного дерева: соответствие
# «бинарь ↔ SHA» снаружи cargo проверить нечем (fingerprint'ы привязаны к пути
# дерева, а mtime переписывает любой checkout/merge). Авторитет — cargo, поэтому
# сборка всегда идёт в слоте; тёплый target/ делает её дешёвой, а кэш по SHA
# убирает повторы. Ошибочный baseline дороже пересборки: он даёт молча неверный
# перф-вывод.

set -u

die() { printf '%s\n' "$*" >&2; exit 1; }

common=$(git rev-parse --git-common-dir 2>/dev/null) || die 'Не git-репозиторий.'
common=$(cd "$common" && pwd) || die "Не читается git-dir: $common"
ROOT=$(dirname "$common")
POOL="$ROOT/.claude/worktrees"
SLOT="$POOL/perf-base"
CACHE_ROOT="$ROOT/.claude/perf-baseline"
KEEP=3   # сколько SHA держим в кэше (бинарь ~79 МБ)

want_tests=0
if [ "${1:-}" = "--tests" ]; then want_tests=1; shift; fi
ref=${1:-main}

sha=$(git rev-parse --verify --quiet "$ref^{commit}") || die "Не разрешается ref: $ref"
short=${sha:0:12}

if [ "$want_tests" = 1 ]; then name=lumen-tests.exe; else name=lumen.exe; fi
cache_dir="$CACHE_ROOT/$short"
cached="$cache_dir/$name"

# --- Кэш-хит ---
if [ -f "$cached" ]; then
  # Освежаем mtime: чистка ниже выселяет по нему, и без touch из кэша вылетал
  # бы самый нужный SHA (tip main запрашивают чаще всего, а создан он раньше).
  touch "$cache_dir" 2>/dev/null
  echo "Кэш: $name для $short уже собран."
  printf '%s\n' "$cached"
  exit 0
fi

echo "Кэша нет: собираю $name для $ref ($short) в слоте perf-base."

# --- Слот perf-base на нужном SHA ---
if [ -f "$SLOT/.git" ]; then
  # Соседний скрипт, а не $ROOT/scripts/: иначе ветка, которая правит пул,
  # молча тестировалась бы против копии из главного дерева.
  # stdout глушим, stderr оставляем — диагностика гардов должна доходить.
  bash "$(dirname "$0")/worktree-pool.sh" release perf-base "$sha" >/dev/null \
    || die "Не удалось припарковать слот perf-base на $short (причина выше)."
else
  echo 'Слот perf-base создаётся впервые (checkout ~62k файлов, 2–4 мин)...'
  git worktree add --detach "$SLOT" "$sha" >/dev/null 2>&1 || die 'git worktree add упал.'
fi

# --- Сборка ---
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
if [ "$want_tests" = 1 ]; then
  ( cd "$SLOT" && cargo test -p lumen-shell --profile dev-release --no-run ) \
    || die 'Сборка тест-бинаря упала.'
  # Имя тест-бинаря содержит хэш метаданных (lumen-<hash>.exe); свежесобранный —
  # самый новый по mtime среди deps/lumen-*.exe.
  built=$(ls -t "$SLOT"/target/dev-release/deps/lumen-*.exe 2>/dev/null | head -1)
else
  ( cd "$SLOT" && cargo build -p lumen-shell --profile dev-release ) \
    || die 'Сборка lumen.exe упала.'
  built="$SLOT/target/dev-release/lumen.exe"
fi
[ -n "${built:-}" ] && [ -f "$built" ] || die 'Собранный бинарь не найден.'

mkdir -p "$cache_dir" || die "Не создаётся $cache_dir"
cp "$built" "$cached" || die 'Не удалось положить бинарь в кэш.'
printf '%s\n' "$sha" > "$cache_dir/SHA"

# --- Чистка кэша: держим KEEP последних SHA ---
if [ -d "$CACHE_ROOT" ]; then
  # shellcheck disable=SC2012  # имена каталогов — hex-SHA, спецсимволов нет
  old=$(ls -1t "$CACHE_ROOT" 2>/dev/null | tail -n +$((KEEP + 1)))
  for d in $old; do
    rm -rf "${CACHE_ROOT:?}/$d" && echo "Кэш: вычищен старый $d"
  done
fi

echo "Готово: $name для $short."
printf '%s\n' "$cached"
