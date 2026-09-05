# BUG-1001 — `lumen-layout`'s debug_assert-only инварианты мертвы под `--profile dev-release -D warnings` (мандатный гейт НЕ задет)

**Статус:** OPEN
**Заведён:** 2026-09-05 (P1, побочная находка при подготовке гейта для FONTLOAD-6)
**Область:** `crates/engine/layout/src/invariants.rs` (`check_geometry`, `check_finite`, `check_containment` — DEVX-8a)
**Владелец:** не назначен (задевает любую сессию, чей крейт зависит от `lumen-layout`, только если она добавляет `--profile dev-release` к гейту)

## Уточнение 2026-09-05 (та же сессия, до коммита FONTLOAD-6)

Первая версия этой находки переоценила блокировку: точная мандатная команда
из `docs/commands.md:18` (`cargo clippy -p lumen-layout --all-targets -- -D
warnings`, **без** `--profile`) **зелёная** — проверено на этой же ветке.
Профиль по умолчанию (`dev`) не наследует `release` и держит
`debug_assertions = true`, поэтому вызовы в `box_tree/entry.rs` (за
`#[cfg(debug_assertions)]`) компилируются и функции не мертвы. Красным гейт
становится только если явно добавить `--profile dev-release` — что и сделала
исходная репродукция ниже, спутав его с мандатной командой. Симптом и причина
ниже реальны и воспроизводимы, просто это не «мандатный гейт красный на
main», а «гейт станет красным для любой сессии, которая когда-нибудь начнёт
гонять клиппи под `--profile dev-release»`» (например, если кто-то решит
проверять линты в профиле, реально идущем в `cargo build`/`cargo run` внутри
слота — `CLAUDE.md`, «Build only dev-release inside a slot» — но это про
`build`/`run`, не про мандатную `clippy`-команду). Не блокировало
FONTLOAD-6: `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` и `cargo clippy -p lumen-shell --all-targets --features v8 --
-D warnings` (точные команды, без `--profile`) оба чисты на этой ветке.

## Симптом (только под `--profile dev-release`, см. уточнение выше)

`cargo clippy -p lumen-layout --all-targets --profile dev-release -- -D warnings`
красный уже на `origin/main` (проверено на
`07e54a7fd`, коммит DEVX-8a/DEVX-11 не менялся с тех пор):

```
error: function `check_geometry` is never used
error: function `check_finite` is never used
error: function `check_containment` is never used
error: could not compile `lumen-layout` (lib) due to 3 previous errors
```

Задевает не только сам `lumen-layout` — любой `cargo clippy -p <crate> --all-targets
-- -D warnings` для крейта, зависящего от `lumen-layout` (`lumen-js`, `lumen-shell`,
и далее по графу), пересобирает `lumen-layout` с `-D warnings` и падает тем же
образом, если он ещё не закэширован без этого флага.

## Причина

`invariants.rs` (DEVX-8a, `1bcf646c6`) — «инвариантный слой geometry/style,
debug_assert-only» по собственному сообщению коммита: `check_geometry`/
`check_finite`/`check_containment` вызываются только изнутри `debug_assert!(...)`.
Ровно это уже задокументировано как ловушка в `CLAUDE.md`/«Known gotchas»:
«`debug_assert!` — не проверка в профиле, который собирают все: `dev-release` и
`release` оба вырезают его целиком». В `dev-release` (мандатный профиль для
гейта) вызовы `debug_assert!(check_geometry(...))` компилируются в ничто,
поэтому сами функции становятся мёртвым кодом — `dead_code` при `-D warnings`
превращается в ошибку компиляции, а не просто предупреждение.

Не воспроизведено на моей ветке — я не трогал `crates/engine/layout`; смотри
`git diff --stat` FONTLOAD-6 (`p1-fontload6-scripted-fontface-registry`) для
подтверждения.

## Почему это не просто «мелкий dead_code»

Ломает мандатный локальный гейт («The local gate is NOT replaced by CI»,
`docs/git-workflow.md`) для потенциально любой сессии P1–P4, чей крейт тянет
`lumen-layout` — то есть почти весь движок. До сих пор не пойман, видимо,
потому что предыдущие клиппи-прогоны переиспользовали уже тёплый `target/`
`lumen-layout`, собранный без `-D warnings` в этой же сборке (кэш не
инвалидировался до этого расследования).

## Первый шаг

Либо обернуть сами функции/их debug_assert-вызовы в `#[cfg(debug_assertions)]`
согласованно (сейчас вызовы компилируются, а функции — нет, что и даёт
рассинхрон), либо завести реальный вызывающий путь вне `debug_assert!`
(например под фичей строгой проверки инвариантов). Второе аккуратнее
соответствует изначальному замыслу DEVX-8a — не просто заглушить
предупреждение.

## Сырые данные

```
cargo clippy -p lumen-layout --all-targets --profile dev-release -- -D warnings
```
на `.claude/worktrees/p1-work` (база — `origin/main` `07e54a7fd`), 2026-09-05.
