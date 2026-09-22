# Задача: RP-9 — Фильтр print-таблиц стилей (не применять media=print CSS при экранном рендере)

**Developer:** P1
**Ветка:** `p1-rp9-print-stylesheet-filter`
**Размер:** S
**Крейты:** `lumen-shell` (+ верификация `lumen-html-parser`, `lumen-layout`)

## Goal

При экранном рендере не применять таблицы стилей, предназначенные только для печати
(`<link rel=stylesheet media=print>` и `@media print { … }`), чтобы print-декорации
(`a::after{content:attr(href)}` и т.п.) не протекали в экранный каскад. Группа RP =
рендер-паритет реального веба (эталон — Edge/Chromium). Семя: BUG-268.

## Current state (сверено с кодом 2026-07-05)

**ВАЖНО: основная задача уже РЕАЛИЗОВАНА (BUG-268 FIXED 2026-07-04, BUG-270 FIXED 2026-07-04).**
`BUGS.md:284` (BUG-268) и `BUGS.md:286` (BUG-270). Что уже есть в коде:

- **Media-гейт `<link>`**: `crates/shell/src/main.rs:3574` `link_media_matches(media, ctx)` —
  пустой/отсутствующий `media` = «all»→применяется; иначе штатный
  `parse_media_query(media).matches(ctx)` (второго матчера нет).
- **`collect_link_hrefs` фильтрует по media**: `crates/shell/src/main.rs:3670`, ключевая
  проверка `crates/shell/src/main.rs:3693-3695` (`link_media_matches(media, media_ctx)`).
  `load_linked_stylesheets` (`main.rs:3610`) принимает `MediaContext` параметром.
- **Экранный контекст**: `crates/shell/src/main.rs:3586` `screen_media_context` →
  `media_type:"screen"`. Print-контекст для PDF: `main.rs:3600` `print_media_context` →
  `media_type:"print"`. Выбор по `media_print` в точке загрузки CSS:
  `crates/shell/src/main.rs:4334-4345`.
- **Каскадный `@media print` фильтр**: `crates/engine/layout/src/style.rs:18618`
  `media_context_from_viewport` — `"print"` только при sticky `print_media_active()`
  (BUG-270), иначе `"screen"`. Юнит `style.rs:20617`
  `bug270_print_media_flag_switches_cascade_media_type`.
- **Preload-путь**: `PreloadHint::Stylesheet` получил поле `media` (preload_scanner);
  не матчащие листы не греют кэш и не шлют `CssLoaded` (см. описание в `BUGS.md:284`).
- **Существующие тесты**: `collect_link_hrefs_media_gate` (shell),
  `link_stylesheet_media_attr_preserved` (html-parser), `bug270_*` (layout). Фикстура с
  `media=print`/`media=screen` линками — `crates/shell/src/main.rs:18471-18472`.

**Задача сводится к аудиту остаточных дыр и добавлению покрытия**, а не к реализации с нуля.
Кандидаты на остаток (проверить по коду при старте):

1. `@media print` внутри **инлайновых `<style>`** — фильтруется каскадом (`@media`-правила),
   но стоит подтвердить тестом «экран НЕ применяет `@media print{}` из `<style>`».
2. Сложные media-строки у `<link>`: `media="print, screen"`, `media="not print"`,
   `media="screen and (min-width:…)"` — убедиться, что `parse_media_query().matches()`
   их гейтит корректно (граничные кейсы).
3. `@import url(...) print;` — `ImportRule.media` (`parser.rs:1095`) существует; проверить,
   применяется ли гейт к `@import` при экранном рендере (потенциальная дыра).

## Entry points

- `crates/shell/src/main.rs:3574` — `link_media_matches` (гейт `<link>`).
- `crates/shell/src/main.rs:3670` / `:3693` — `collect_link_hrefs` (фильтр href по media).
- `crates/shell/src/main.rs:3586` / `:3600` — screen/print `MediaContext`.
- `crates/engine/layout/src/style.rs:18618` — `media_context_from_viewport` (каскадный `@media`).
- `crates/engine/css-parser/src/parser.rs:1086-1095` — `ImportRule` + его `media` (кандидат на дыру).

## Срезы (декомпозиция)

### Срез 1 — XS — аудит остаточных дыр
Пройти 3 кандидата выше по коду; для каждой зафиксировать: закрыта / дыра. Если всё закрыто —
задача превращается в «closure + тесты» (Срез 3). Если найдена дыра (вероятнее всего `@import`)
— завести/переиспользовать BUG-номер и чинить в Срезе 2.

### Срез 2 — XS/S — закрыть найденную дыру (условно)
Наиболее вероятная — `@import url(...) print;` при экранном рендере: применить тот же
`MediaContext`-гейт, что и для `<link>`, к `ImportRule.media` в месте разворачивания
`@import` (найти по grep `ImportRule`/`import` в layout/style.rs). Если дыр нет — срез пустой.

### Срез 3 — XS — покрытие тестами
Добавить недостающие юнит-тесты: `@media print` в инлайновом `<style>` не влияет на экранный
каскад; `media="not print"` / `media="print, screen"` у `<link>` гейтятся верно. По
возможности — граф-тест на класс BUG-268 (print-декор `a::after{content:attr(href)}` не виден
на экране) в `graphic_tests/`.

### Срез 4 — XS — доки
`CAPABILITIES.md` (media-квери/print), при закрытии дыры — `BUGS.md` (OPEN→FIXED).
`ROADMAP.md` RP-9 (не в этой задаче — трогает P-владелец при merge).

## Tests

- Юнит (shell): экран НЕ грузит `media=print` link (уже есть `collect_link_hrefs_media_gate` —
  расширить кейсами `not print` / `print, screen`).
- Юнит (layout): `@media print{}` из инлайнового `<style>` не применяется на экране.
- Юнит (условно): `@import ... print;` не применяется на экране.
- Graphic-тест (по возможности): print-only декорации не рендерятся на экране.

## Definition of done

- [x] Аудит 3 кандидатов проведён; статус каждой дыры зафиксирован.
- [x] `@import ... print` (если дыра) — гейтится тем же `MediaContext`.
- [x] Добавлены тесты на граничные media-строки `<link>` и на `@media print` в `<style>`.
- [x] Никаких регрессий на print-в-PDF (BUG-270 путь) — `bug270_*` тест зелёный.
- [x] Доки обновлены; если закрыта дыра — соответствующий BUG помечен FIXED.

## Итог (2026-09-22, P1)

Все 3 кандидата на дыры уже были закрыты в коде до старта задачи — дыр не
найдено, Срез 2 пуст:

1. **Инлайновый `@media print`** — фильтруется на общем пути каскада
   (`CascadeIndex::build`, `crates/engine/layout/src/style/cascade_index.rs:122`,
   `sheet.media_rules.iter().map(|m| m.query.matches(media_ctx))`), не только
   у `<link>` — `media_ctx` строится из того же sticky-флага `print_media_active()`
   (`style/env.rs`) независимо от источника листа.
2. **Сложные media-строки `<link>`** (`not print`, `print, screen`,
   `screen and (min-width: …)`) — `MediaQuery::matches`/`MediaQueryClause::matches`
   (`crates/engine/css-parser/src/parser/media.rs:589-621`) уже реализуют полный
   OR/AND/`not`/`only` по Media Queries L4 §3.
3. **`@import url(...) print;`** — гейтится тем же `MediaContext` в
   `inline_css_imports` (`crates/shell/src/stylesheets.rs:380`,
   `if !imp.media.matches(media_ctx) { continue; }`), покрыт тестом
   `inline_css_imports_media_gate` (уже существовал).

Добавлено покрытие тестами (Срез 3), которого не хватало:

- `media_print_block_is_screen_gated_in_cascade`
  (`crates/engine/layout/src/style/tests/mod.rs`) — сквозной `compute_style`
  тест: `@media print{}` не применяется на экране и применяется при
  `set_print_media(true)`, дополняет `bug270_*` (тот проверял только флаг).
- `collect_link_hrefs_media_gate_compound_queries`
  (`crates/shell/src/tests/page_resources.rs`) — `not print`/`not screen`/
  `print, screen`/`print, (min-width: …)` у `<link>`.

Срез 4 (доки): этот файл обновлён; `ROADMAP.md` RP-9 помечена `done`
(закрытие делает P1 при мерже, задача не блокирует BUG — BUG-268/270 уже
`FIXED`, новых BUG не заведено).
