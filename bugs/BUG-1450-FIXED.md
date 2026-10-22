# BUG-1450 — `:dir(rtl)` не распознаёт направление по тексту: `dir=auto` на `<div>`, `<input>` и `<bdi>` с текстом на иврите даёт `ltr`

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** layout (`crates/engine/layout/src/style/matching/forms.rs` → `matches_dir`/`element_directionality`)

## Симптом

`<bdi>שלום</bdi>`, `<div dir=auto>שלום</div>`, `<input dir=auto value="שלום">`: `el.matches(':dir(rtl)')` = `false` (для `<input>` `:dir(ltr)` = `true`), правило `:dir(rtl){color:green}` не применяется. Спецификация (HTML §3.2.6 «auto directionality», Selectors 4 §10.3): `<bdi>` без `dir` и `dir=auto` определяют направление по первому сильному символу содержимого/значения. Страдают 6 id (`dir-pseudo-on-bdi-element`, `dir-selector-auto`, `dir-pseudo-on-input-element`, `invalidation/part-dir`, `dir-style-02a`, `dir-selector-change-004`); 25 из 49 упавших сабтестов (по сообщениям и именам файлов; пробой подтверждены `<bdi>`, `dir=auto` у `<div>` и `<input>`).

## Проба

| разметка | `matches(':dir(rtl)')` | ожидается |
|---|---|---|
| `<bdi>שלום</bdi>` | `false` | `true` |
| `<div dir=auto>שלום</div>` | `false` | `true` |
| `<input dir=auto value="שלום">` | `false` (`:dir(ltr)` `true`) | `true` |
| `<div dir=rtl>` (контроль) | `true` | верно |

## Как найдено

WPT-RUN-14 срез 20: `selectors/dir-pseudo-on-bdi-element.html` (3 из 5), `dir-selector-auto.html` (8 из 22), `dir-pseudo-on-input-element.html` (12 из 20).

## Что делать

Реализовать определение направления `auto` (HTML §3.2.6.1) и использовать его в `matches_dir`; динамическое обновление при смене текста/`dir` — отдельно (`dir-selector-change-004`).

## Как проверить

Таблица выше; `css/selectors/dir-pseudo-on-bdi-element.html`.

## Исправление

`element_directionality` (`style/matching/forms.rs`) реализует HTML LS §3.2.6.1: `dir=ltr|rtl` — значение; `dir=auto` и `<bdi>` без валидного `dir` — первый сильный символ (UAX #9 P2/P3): у `<textarea>` и `<input>` с текстовым типом (всё, кроме `date`/`time`/`datetime-local`/`month`/`week`/`number`/`range`/`color`/`checkbox`/`radio`/`image`/`file`) — в `Document::control_value`, у остальных — в тексте поддерева (`first_strong_direction`, пропускает `script`/`style`/`textarea`/`bdi` и потомков с валидным `dir`); нет сильных → `ltr`. `<input type=tel>` без валидного `dir` — всегда `ltr`. Невалидный `dir` теперь наследует направление родителя, а не даёт `ltr`. Проба из таблицы (JS `matches(':dir(rtl)')`, включая динамическую смену `type`/`value`/`dir`) — все строки верны; юнит-тесты `dir_*` в `tests/state_pseudo_classes.rs`.

Не сделано: свойство `direction` в `getComputedStyle` для `<bdi>`/`<input dir=auto>` (presentational hint учитывает только `dir` на элементе и не знает про `bdi`/`tel`), инвалидация стилей при смене текста у `dir=auto` (`dir-selector-change-004`).
