#!/usr/bin/env bash
# Взять задачу: проверить занятость по origin, занять слот пула, опубликовать
# ветку-резервацию и перепроверить гонку.
#
# Зачем: резервация задачи — это ветка `p<N>-…` на origin (docs/dev-roles.md
# §Reserving a task). Прежняя проверка (`git pull origin main; git branch -a`)
# пропускала чужую бронь трижды: `pull origin main` не обновляет
# remote-tracking ветки, поэтому `branch -a` показывает origin на момент
# последнего полного fetch; сверялось точное имя `p<N>-<задача>`, а другая
# машина могла назвать ветку иначе (`p1-bug1240-relative`); пуш ветки
# откладывался или забывался. Так BUG-1240 2026-10-07 взяли на двух машинах.
#
# Использование:
#   bash scripts/task-claim.sh <N> <task-id> [branch]
#   bash scripts/task-claim.sh --check <N> <task-id>   # только проверка, без слота и пуша
#     N       — номер разработчика (1…6); слот пула p<N>-work
#     task-id — BUG-1240 / 1240 / ROADMAP-id (perf-10, wpt-run14-s15 …)
#     branch  — имя ветки; по умолчанию p<N>-bug-<num> или p<N>-<task-id>
#
# Задача считается занятой, если на origin или локально есть ветка, в имени
# которой встречается её id, и эта ветка не влита в origin/main. «Влита» =
# её вершина достижима из origin/main, но не лежит на first-parent цепочке
# main (то есть пришла через merge-коммит). Свежая ветка-резервация без
# коммитов стоит прямо на first-parent цепочке и потому считается живой.
#
# Код возврата: 0 — задача взята (последняя строка вывода — путь слота, для
# `cd "$(... | tail -1)"`); 1 — занята или ошибка; 2 — гонка: после пуша на
# origin оказалась ещё одна ветка с тем же id (решает пользователь).

set -u

die() { printf '%s\n' "$*" >&2; exit 1; }

check_only=0
[ "${1:-}" = --check ] && { check_only=1; shift; }
n=${1:-}
id=${2:-}
[ -n "$n" ] && [ -n "$id" ] || die 'Использование: task-claim.sh <N> <task-id> [branch]'
case "$n" in [1-9]) ;; *) die "Номер разработчика — одна цифра, получено '$n'." ;; esac

lower=$(printf '%s' "$id" | tr '[:upper:]' '[:lower:]')
if num=$(printf '%s' "$lower" | sed -nE 's/^(bug-?)?([0-9]+)$/\2/p') && [ -n "$num" ]; then
  # Баг: номер с цифровыми границами — ловит bug-1240, bug1240-relative, 1240-fix
  pattern="(^|[^0-9])${num}([^0-9]|$)"
  default_branch="p${n}-bug-${num}"
else
  esc=$(printf '%s' "$lower" | sed -E 's/[.[\*^$()+?{}|]/\\&/g')
  pattern="(^|[-/_])${esc}([-/_]|$)"
  default_branch="p${n}-${lower}"
fi
branch=${3:-$default_branch}
case "$branch" in "p${n}-"*) ;; *) die "Ветка должна начинаться с p${n}- (получено '$branch')." ;; esac

ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || die 'Не git-репозиторий.'
cd "$ROOT" || die "Не удалось перейти в $ROOT"

fetch() { git fetch origin --prune --quiet || die 'git fetch origin не удался — без свежего origin проверка занятости бессмысленна.'; }

# Ветки (имя без refs/heads/ и origin/), совпадающие с id, кроме своей и влитых
live_matches() {
  local fp
  fp=$(git rev-list --first-parent origin/main)
  {
    git for-each-ref --format='%(refname:short) %(objectname)' refs/heads
    git for-each-ref --format='%(refname:short) %(objectname)' refs/remotes/origin \
      | sed 's#^origin/##' | grep -v '^HEAD '
  } | while read -r name sha; do
    [ "$name" = "$branch" ] && continue
    [ "$name" = main ] && continue
    printf '%s\n' "$name" | tr '[:upper:]' '[:lower:]' | grep -qE "$pattern" || continue
    if git merge-base --is-ancestor "$sha" origin/main 2>/dev/null \
       && ! printf '%s\n' "$fp" | grep -qx "$sha"; then
      continue  # влита через merge-коммит — история, не бронь
    fi
    printf '%s\n' "$name"
  done | sort -u
}

fetch

# 1. Чужие ветки с тем же id
busy=$(live_matches)
if [ -n "$busy" ]; then
  echo "Задача $id уже занята — найдены невлитые ветки:" >&2
  printf '  %s\n' $busy >&2
  die 'Не дублируй работу: возьми следующую строку STATUS или спроси пользователя.'
fi

# 2. Своё имя ветки уже на origin, а локально его нет — значит, его заняла
#    другая машина под тем же именем.
local_has=0; remote_has=0
git show-ref --verify --quiet "refs/heads/$branch" && local_has=1
git show-ref --verify --quiet "refs/remotes/origin/$branch" && remote_has=1
if [ "$remote_has" = 1 ] && [ "$local_has" = 0 ]; then
  die "Ветка $branch уже есть на origin, но не здесь — задача взята на другой машине."
fi
if [ "$check_only" = 1 ]; then
  echo "Задача $id свободна (ветка $branch)."
  exit 0
fi

# 3. Слот пула
path=$(bash scripts/worktree-pool.sh "p${n}-work" "$branch" | tail -1)
[ -n "$path" ] && [ -d "$path" ] || die 'worktree-pool.sh не занял слот (см. сообщение выше).'

# 4. Публикация = резервация. Отказ пуша (на origin другая история) — стоп.
git push --quiet origin "$branch" 2>&1 | grep -v '^remote:' >&2
[ "${PIPESTATUS[0]}" = 0 ] || die "git push origin $branch отклонён — разберись, чья это ветка, прежде чем работать."

# 5. Перепроверка гонки: кто-то мог опубликовать бронь между шагами 1 и 4.
fetch
race=$(live_matches)
if [ -n "$race" ]; then
  echo "ГОНКА: после публикации $branch на origin есть ещё ветки с id $id:" >&2
  printf '  %s\n' $race >&2
  echo "Код не трогай, спроси пользователя, кто продолжает задачу." >&2
  printf '%s\n' "$path"
  exit 2
fi

echo "Задача $id зарезервирована: ветка $branch опубликована, слот p${n}-work."
printf '%s\n' "$path"
