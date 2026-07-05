# Задача: View Transitions для cross-document навигации (MPA)

**Developer:** P1
**Ветка:** `p1-view-transitions-mpa`
**Размер:** M
**Крейты:** `lumen-css-parser`, `lumen-layout`, `lumen-shell`, `lumen-js`

## Goal

Реализовать cross-document (MPA) View Transitions L2: при навигации между документами того же
origin, если оба документа объявили `@view-transition { navigation: auto; }`, показать
анимированный переход (снимок старой страницы → cross-fade → новая), переиспользуя уже
существующий same-document (SPA) движок переходов. Спек: CSS View Transitions Module Level 2.

## Current state (сверено с кодом 2026-07-05)

Same-document (SPA) переходы РЕАЛИЗОВАНЫ и являются базой для переиспользования:

- **JS API `document.startViewTransition`** — полноценный shim с промисами/отменой:
  `crates/js/src/view_transitions.rs:38` (`VIEW_TRANSITION_SHIM`),
  `crates/js/src/view_transitions.rs:90` (`install_view_transition_bindings`).
- **Событийный мост в шелл** — `enum ViewTransitionEvent { Begin, End, Cancel }`
  (`crates/js/src/view_transitions.rs:19`); native `_lumen_vt_begin/_end/_cancel` пушат
  события в `Arc<Mutex<Vec<..>>>`, которые шелл **дренит в `about_to_wait`** и гонит cross-fade
  (док-комментарий `view_transitions.rs:5-6`, `:85-89`). Именно этот путь (snapshot старого
  кадра → relayout → 300ms cross-fade) — то, что нужно переиспользовать для MPA.
- **`::view-transition-*` псевдоэлементы / `view-transition-name`** обрабатываются в layout/paint
  (grep `view_transition` даёт совпадения в `crates/engine/layout/src/style.rs`,
  `crates/engine/layout/src/lib.rs`, `crates/engine/paint/src/backend.rs`,
  `crates/engine/css-parser/src/lib.rs`).

Чего НЕТ (нужно сделать):

- **At-rule `@view-transition { navigation: auto }` не парсится** — grep по `view-transition`
  в css-parser показывает только `view-transition-name`/псевдоэлементы, самого at-rule нет
  (нет `ViewTransitionRule` рядом с `MediaRule`/`PageRule` в `crates/engine/css-parser/src/parser.rs`).
- **Нет перехвата навигации** для запуска перехода: шелл при переходе на новый URL сразу
  парсит/раскладывает новую страницу без snapshot старой (навигационный пайплайн —
  `parse_and_layout` в `crates/shell/src/main.rs`, порядка `main.rs:4300+`), сигнал
  «начать VT» на границе документов не отправляется.
- **Нет reveal-хука в новом документе** — старый snapshot должен доживать до готовности нового
  layout, затем стартует тот же cross-fade, что и в SPA.

Итог: SPA-переходы готовы и переиспользуемы; MPA = at-rule + перехват навигации + прокидка
snapshot между документами через уже существующий cross-fade движок шелла.

## Entry points

- `crates/js/src/view_transitions.rs:19` — `ViewTransitionEvent` (переиспользуемый мост).
- `crates/js/src/view_transitions.rs:90` — установка биндингов (шаблон дренажа событий).
- `crates/engine/css-parser/src/parser.rs` — рядом с `MediaRule` (`:1146`) / `PageRule`
  (`:929`) добавить `ViewTransitionRule` + at-rule парсинг.
- `crates/shell/src/main.rs` (`parse_and_layout`, ~`:4300+`) — навигационный пайплайн, точка
  перехвата «старый документ → новый».
- `crates/shell/src/main.rs` (`about_to_wait`, дренаж `ViewTransitionEvent`) — существующий
  cross-fade драйвер, к которому подключается MPA-переход.

## Срезы (декомпозиция)

### Срез 1 — S — парсинг at-rule `@view-transition`
Добавить `ViewTransitionRule { navigation: Navigation }` (`Auto`/`None`) в parser.rs рядом с
`MediaRule`/`PageRule`; распознать блок `@view-transition { navigation: auto; }` в at-rule
свитче парсера; сложить в `Stylesheet`. Юнит-тесты на парсинг (auto/none/отсутствие).

### Срез 2 — XS — извлечение opt-in из документа
В шелле после парсинга каждого документа определить, объявлен ли `navigation: auto` (helper
над `Stylesheet.view_transition_rules`). Кэшировать флаг для текущей и для новой страницы —
переход стартует только если **оба** документа opt-in и same-origin (спек L2 §navigation).

### Срез 3 — S — snapshot старого документа при навигации
На границе навигации (перед заменой на новый layout, `parse_and_layout` ~`main.rs:4300+`),
если исходящая страница opt-in, захватить display-list/кадр старого документа — точно так же,
как SPA-путь захватывает по `ViewTransitionEvent::Begin`. Сохранить в поле состояния окна
(«pending MPA snapshot»).

### Срез 4 — S — reveal нового документа через существующий cross-fade
После готовности layout нового документа: если оба opt-in — запустить тот же cross-fade
драйвер, что дренит `ViewTransitionEvent::End` в `about_to_wait`, подсунув pending-snapshot
как «старый кадр». Переиспользовать существующий таймер/интерполяцию (300ms), НЕ писать второй.

### Срез 5 — XS — отмена/фолбэк
Если новый документ не opt-in / cross-origin / snapshot протух — сбросить pending-snapshot и
навигировать без анимации (fallback). Аналог `ViewTransitionEvent::Cancel`.

### Срез 6 — XS — доки/тесты
`CAPABILITIES.md`, `CSS-SPECS.md` (View Transitions L2), `subsystems/*`. Graphic-тест по
шаблону same-document VT (см. `project_test61_view_transitions_debtor` — MPA может стать
KNOWN_DEBTOR из-за async-тайминга Edge).

## Tests

- Юнит (css-parser): `@view-transition { navigation: auto }` → `ViewTransitionRule{Auto}`;
  `navigation: none` → `None`; кривой блок игнорируется.
- Юнит (shell): helper opt-in даёт true только при обоих opt-in + same-origin.
- Юнит (shell): pending-snapshot ставится/сбрасывается по срезам 3/5.
- Graphic/interaction-тест: навигация между двумя локальными страницами с `@view-transition`
  показывает cross-fade (по образцу `graphic_tests` VT-теста); при необходимости — KNOWN_DEBTOR.

## Definition of done

- [ ] `@view-transition { navigation: auto/none }` парсится в `Stylesheet`.
- [ ] Same-origin навигация с двусторонним opt-in запускает cross-fade **через существующий
      SPA-движок** (нового драйвера не заведено).
- [ ] Cross-origin / односторонний opt-in / ошибка snapshot → навигация без анимации.
- [ ] SPA `startViewTransition` не задет — регрессий нет.
- [ ] Юнит + graphic/interaction-тест зелёные (или обоснованный KNOWN_DEBTOR); доки обновлены.
