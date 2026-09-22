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

### Срез 1 — S — парсинг at-rule `@view-transition` — DONE
`ViewTransitionRule { navigation: ViewTransitionNavigation }` (`Auto`/`None`) в
`css-parser/src/parser/at_rules.rs`, рядом с `MediaRule`/`PageRule`; `Stylesheet.view_transition_rules`.
7 юнит-тестов в новом `parser/tests/view_transitions.rs` (`at_rules.rs` уже на пределе лимита
2000 строк).

### Срез 2 — XS — извлечение opt-in из документа — DONE
`page_pipeline::view_transition_navigation_opted_in` (последнее объявление в документе
побеждает) + `mpa_view_transition_allowed` (same-origin + оба документа opt-in). Пока не
подключены к навигационному пайплайну — `#[allow(dead_code)]`, 6 юнит-тестов в
`tests/page_pipeline.rs`. **Не кэшируется** на `LoadedPage`/`LayoutSource` — это часть среза 3
(нужно решить, где именно живёт флаг исходящей страницы на границе навигации).

### Срез 3 — S — snapshot старого документа при навигации — DONE
На границе навигации — `Lumen::navigate_to_inner`/`navigate_replace`
(`crates/shell/src/lumen/navigation.rs`), перед `self.source = source` —
новый метод `maybe_capture_mpa_view_transition_snapshot` проверяет исходящую
страницу через новую чистую функцию `page_pipeline::mpa_view_transition_departure_candidate`
(same-origin + `view_transition_navigation_opted_in` исходящего стилшита;
входящий документ ещё не загружен, его opt-in проверит срез 4) и, если
кандидат, клонирует `self.display_list` в новое поле состояния окна
`Lumen::pending_mpa_view_transition_snapshot` — точно так же, как SPA-путь
захватывает по `ViewTransitionEvent::Begin`. 3 юнит-теста в
`tests/page_pipeline.rs` (`departure_candidate_*`); Lumen-глю не тестируется
юнитом за отсутствием тестового конструктора `Lumen` — логика решения
покрыта через чистую функцию.

### Срез 4 — S — reveal нового документа через существующий cross-fade — DONE
`Lumen::maybe_reveal_mpa_view_transition` (`crates/shell/src/lumen/navigation.rs`), вызывается
из обоих путей загрузки страницы (`apply_loaded_page` — стриминговый, и синхронный fallback в
`page_load.rs::reload`) сразу после `set_display_list`, когда `self.layout_source` уже
указывает на новый документ. Берёт `pending_mpa_view_transition_snapshot` (`.take()`),
проверяет `view_transition_navigation_opted_in` на входящем стилшите и, если он тоже opt-in,
заводит `Lumen::view_transition = Some(ViewTransitionState{ old_dl, start_ms: now, duration_ms:
300.0 })` — тот же `ViewTransitionState`, что `about_to_wait` заводит для SPA-пути по
`ViewTransitionEvent::End`; `redraw_requested.rs` дальше блендит его с новым кадром без
изменений. Не перепроверяет same-origin повторно: он уже решён на границе навигации
(`mpa_view_transition_departure_candidate`, срез 3) и исходящий документ к моменту reveal уже
выгружен, так что второй раз сверить не с чем — см. комментарий на
`page_pipeline::mpa_view_transition_allowed`.

### Срез 5 — XS — отмена/фолбэк — DONE
Не opt-in / cross-origin уже отсекаются на границе капчи (срез 3, same-origin +
`mpa_view_transition_departure_candidate`) и на reveal (срез 4, opt-in входящего документа) —
`maybe_reveal_mpa_view_transition` в обоих случаях просто не заводит `view_transition`,
навигация остаётся без анимации без отдельного кода. Оставался один незакрытый путь —
**протухший snapshot**: если `maybe_capture_mpa_view_transition_snapshot` захватил кадр
исходящего документа, но сама навигация не долетает до `apply_loaded_page`/
`maybe_reveal_mpa_view_transition` (сетевая ошибка), `pending_mpa_view_transition_snapshot`
раньше повисал в `Some` до следующего вызова `navigate_to_inner`/`navigate_replace` (тот сбрасывает
его первой строкой) — окно есть, где последующий **reload** того же упавшего URL (не проходит
через navigate_to_inner) мог бы всплыть с устаревшим snapshot от давно ушедшей страницы. Сброс
`pending_mpa_view_transition_snapshot = None` добавлен на все три пути ошибки навигации:
`LoadEvent::RenderDone` (`Err` рукав, `crates/shell/src/app/user_event.rs`),
`LoadEvent::LoadError` (там же) и синхронный fallback `reload()`'s `Err` рукав
(`crates/shell/src/page_load.rs`).

### Срез 6 — XS — доки/тесты — DONE
`CAPABILITIES.md` (Misc-строка), `CSS-SPECS.md` (новая строка View Transitions L2, `#58`),
`subsystems/shell.md` (Done-запись с инвариантом среза 5) обновлены. Юнит-тесты не добавлялись —
они уже landed по срезам (7 css-parser + 10 opt-in-helper'ов page_pipeline + 3
`departure_candidate_*`, все зелёные, перепроверено). SPA `startViewTransition`
(`crates/js/src/view_transitions.rs`) не тронут ни одним из срезов 1-5 (grep подтверждает —
все изменения только в `css-parser/parser/at_rules.rs`, `shell/page_pipeline.rs`,
`shell/lumen/{navigation,state}.rs`, `shell/app/user_event.rs`, `shell/page_load.rs`), так что
регрессии по построению нет. Graphic/interaction-тест **не заведён**: `graphic_tests` — это
детерминированный однодокументный пайплайн (`parse_and_layout` один раз → скриншот), у него нет
харнеса «навигация между двумя документами», а `KNOWN_DEBTORS`-ратчет рассчитан на статичный
пиксельный baseline одной страницы, которого здесь нет. Реальная проверка cross-fade между двумя
документами требует внешнего E2E-стенда (см. память `project_e2e_track_external_stand`) — не
заводим здесь фиктивный тест ради галочки.

## Tests

- Юнит (css-parser): `@view-transition { navigation: auto }` → `ViewTransitionRule{Auto}`;
  `navigation: none` → `None`; кривой блок игнорируется.
- Юнит (shell): helper opt-in даёт true только при обоих opt-in + same-origin.
- Юнит (shell): pending-snapshot ставится/сбрасывается по срезам 3/5.
- Graphic/interaction-тест: навигация между двумя локальными страницами с `@view-transition`
  показывает cross-fade (по образцу `graphic_tests` VT-теста); при необходимости — KNOWN_DEBTOR.

## Definition of done

- [x] `@view-transition { navigation: auto/none }` парсится в `Stylesheet` (срез 1, landed).
- [x] Opt-in helper (`view_transition_navigation_opted_in`/`mpa_view_transition_allowed`, срез 2,
      landed) — same-origin + двусторонний opt-in, пока не подключён к навигации (срезы 3-4).
- [x] Same-origin навигация с двусторонним opt-in запускает cross-fade **через существующий
      SPA-движок** (нового драйвера не заведено) — срез 4, landed.
- [x] Cross-origin / односторонний opt-in / ошибка snapshot → навигация без анимации (срез 5, landed).
- [x] SPA `startViewTransition` не задет — регрессий нет (срез 6: подтверждено grep + перепрогон тестов).
- [x] Юнит-тесты зелёные (21/21); доки обновлены. Graphic/interaction-тест обоснованно не заведён —
      см. срез 6.
