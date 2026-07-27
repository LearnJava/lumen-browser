#!/usr/bin/env bash
# Пул постоянных worktree: слот на разработчика вместо add+remove на задачу.
#
# Зачем: `git worktree remove` уносит вместе с каталогом прогретый `target/`
# (dev-release ≈ 4.7 ГБ), и следующая задача в новом worktree платит холодную
# сборку 9–15 мин (docs/build-speed.md §7, сценарий S3) плюс сам `git worktree
# add` — на этом репозитории это 62 291 файл и больше 3 минут. Слот
# переиспользуется: меняется только ветка, `target/` остаётся тёплым, и
# пересборка становится инкрементальной (единицы секунд — десятки секунд).
#
# Это НЕ общий `CARGO_TARGET_DIR` на все worktree (отвергнут, docs/build-speed.md
# §6): у каждого слота свой `target/`, target-lock не сериализует параллельные
# сессии.
#
# Использование:
#   bash scripts/worktree-pool.sh <slot> <branch> [base]   # занять слот веткой
#   bash scripts/worktree-pool.sh release <slot> [base]    # освободить слот
#   bash scripts/worktree-pool.sh list                     # что сейчас в слотах
#
#   slot   — каталог в .claude/worktrees/: p1-work … p5-work, perf-base
#   branch — ветка задачи (p<N>-<task>); существующая — переключаемся на неё,
#            новая — создаётся от base
#   base   — база для новой ветки, по умолчанию main
#
# `release` переводит слот на detached HEAD базы: пока слот держит ветку,
# `git branch -d` отказывает («used by worktree»), поэтому шаг «удалить ветку»
# из чеклиста завершения выполняется ПОСЛЕ release. Слот и его тёплый `target/`
# при этом остаются на месте — в отличие от `git worktree remove`.
# На `main` слот встать не может: main занят главным деревом, поэтому detached.
#
# Последняя строка вывода — абсолютный путь слота (для `cd "$(... | tail -1)"`).
#
# Гарды (ничего не затирается молча):
#   1. незакоммиченная работа в слоте → отказ (шли `git commit` или разбирайся);
#   2. свои коммиты, не влитые в base → отказ (сначала влей или удали ветку);
#   3. прерванный `git worktree add` (есть index.lock, нет index, своих коммитов
#      нет) → чиним сами: это не работа, а недоделанный checkout.

set -u

die() { printf '%s\n' "$*" >&2; exit 1; }

# Главное рабочее дерево, даже если скрипт запущен из worktree:
# `--git-common-dir` указывает на <main>/.git.
common=$(git rev-parse --git-common-dir 2>/dev/null) || die 'Не git-репозиторий.'
common=$(cd "$common" && pwd) || die "Не читается git-dir: $common"
ROOT=$(dirname "$common")
POOL="$ROOT/.claude/worktrees"

# gitdir зарегистрированного worktree (или пусто, если каталог им не является)
slot_gitdir() {
  local p="$1" gd
  [ -f "$p/.git" ] || return 1
  gd=$(sed -n 's/^gitdir: *//p' "$p/.git" | head -1)
  [ -n "$gd" ] && [ -d "$gd" ] && printf '%s\n' "$gd"
}

if [ "${1:-}" = "list" ]; then
  printf '%-12s %-30s %-7s %s\n' SLOT BRANCH DIRTY TARGET
  found=0
  for d in "$POOL"/*-work "$POOL"/perf-base; do
    [ -d "$d" ] || continue
    found=1
    b=$(git -C "$d" branch --show-current 2>/dev/null)
    [ -n "$b" ] || b='(detached)'
    n=$(git -C "$d" status --porcelain 2>/dev/null | wc -l | tr -d ' ')
    t=cold
    [ -d "$d/target" ] && t=warm
    printf '%-12s %-30s %-7s %s\n' "$(basename "$d")" "$b" "$n" "$t"
  done
  [ "$found" = 1 ] || echo '(слотов нет — первый создаётся при первом вызове)'
  exit 0
fi

mode=occupy
if [ "${1:-}" = "release" ]; then
  mode=release
  slot=${2:-}
  base=${3:-main}
  branch=''
  [ -n "$slot" ] || die 'Использование: worktree-pool.sh release <slot> [base]'
else
  slot=${1:-}
  branch=${2:-}
  base=${3:-main}
  [ -n "$slot" ] && [ -n "$branch" ] \
    || die 'Использование: worktree-pool.sh <slot> <branch> [base] | release <slot> | list'
fi
case "$slot" in
  *[!a-z0-9-]* | '' ) die "Недопустимое имя слота: '$slot' (только a-z, 0-9, дефис)" ;;
esac

path="$POOL/$slot"
git show-ref --verify --quiet "refs/heads/$base" || die "Базовая ветка '$base' не найдена."

branch_exists() { git show-ref --verify --quiet "refs/heads/$1"; }

# --- Слота ещё нет: создаём (единственный раз, ~3 мин на этом репозитории) ---
if ! gd=$(slot_gitdir "$path"); then
  [ "$mode" = release ] && die "Слот $slot не существует — освобождать нечего."
  [ -e "$path" ] && die "Каталог $path существует, но это не зарегистрированный worktree.
Осиротевший остаток: проверь 'git worktree list', при необходимости
'git worktree prune' и удали каталог вручную."
  echo "Слот $slot создаётся впервые (checkout ~62k файлов, 2–4 мин)..."
  if branch_exists "$branch"; then
    git worktree add "$path" "$branch" || die 'git worktree add упал.'
  else
    git worktree add "$path" -b "$branch" "$base" || die 'git worktree add упал.'
  fi
  echo "Слот $slot занят веткой $branch (target/ пуст — первая сборка холодная)."
  printf '%s\n' "$path"
  exit 0
fi

# --- Гард 3: прерванный checkout (недоделанный `worktree add`) ---
own_commits() { git -C "$path" log --oneline "$base..HEAD" 2>/dev/null | wc -l | tr -d ' '; }

if [ -f "$gd/index.lock" ] && [ ! -f "$gd/index" ]; then
  if [ "$(own_commits)" != 0 ]; then
    die "В $path есть index.lock и свои коммиты — не трогаю. Разберись вручную."
  fi
  echo "Обнаружен прерванный checkout (index.lock без index) — восстанавливаю..."
  rm -f "$gd/index.lock"
  git -C "$path" reset --hard HEAD >/dev/null 2>&1 || die 'Восстановление не удалось.'
  echo 'Восстановлено.'
fi

# --- Гард 1: незакоммиченная работа ---
dirty=$(git -C "$path" status --porcelain 2>/dev/null)
if [ -n "$dirty" ]; then
  echo "В слоте $slot есть незакоммиченная работа — слот НЕ переключён:" >&2
  printf '%s\n' "$dirty" | head -20 >&2
  n=$(printf '%s\n' "$dirty" | wc -l | tr -d ' ')
  [ "$n" -gt 20 ] && echo "  ... всего $n записей" >&2
  die "Закоммить (git -C \"$path\" commit -am 'wip: ...') или разгреби, потом повтори."
fi

# --- Гард 2: свои коммиты, не влитые в base ---
cur=$(git -C "$path" branch --show-current 2>/dev/null)
ahead=$(own_commits)
if [ "$ahead" != 0 ]; then
  echo "В слоте $slot ветка '${cur:-detached}' имеет $ahead коммит(ов) вне '$base':" >&2
  git -C "$path" log --oneline "$base..HEAD" | head -10 >&2
  die "Влей их или удали ветку, потом повтори — переключение потеряло бы их из вида."
fi

# --- Освобождение слота ---
if [ "$mode" = release ]; then
  git -C "$path" checkout -q --detach "$base" || die "Не удалось перевести слот на detached $base."
  echo "Слот $slot освобождён: detached HEAD на $base (ветка '${cur:-detached}' больше не занята)."
  [ -n "$cur" ] && echo "Теперь можно: git branch -d $cur"
  printf '%s\n' "$path"
  exit 0
fi

# --- Переключение ---
if [ "$cur" = "$branch" ]; then
  echo "Слот $slot уже на ветке $branch."
else
  if branch_exists "$branch"; then
    git -C "$path" checkout -q "$branch" || die "Не удалось переключиться на $branch."
    echo "Слот $slot переключён на существующую ветку $branch."
  else
    git -C "$path" checkout -q -b "$branch" "$base" || die "Не удалось создать $branch от $base."
    echo "Слот $slot: создана ветка $branch от $base."
  fi
fi

[ -d "$path/target" ] && echo 'target/ на месте — сборка инкрементальная.' \
                      || echo 'target/ пуст — первая сборка в этом слоте холодная.'
printf '%s\n' "$path"
