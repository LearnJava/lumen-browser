# BUG-1374 — `is_first_letter_punctuation` не знает Unicode-классов пунктуации (Ps/Pe/Pi/Pf/Po)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** layout (`crates/engine/layout/src/box_tree/pseudo_text.rs::is_first_letter_punctuation` — только ASCII и 10 кавычек)

## Симптом

`is_first_letter_punctuation` (`pseudo_text.rs:91`): `c.is_ascii_punctuation() || matches!(c, '«' | '»' | '“' | '”' | '‘' | '’' | '„' | '‚' | '‹' | '›')`; в комментарии — «no Unicode tables yet». Любой другой знак пунктуации (U+0F3D `༽`, U+2769 `❩`, U+169C `᚜` …) считается буквой и сам становится `::first-letter`.

`--dump-display-list`, `div::first-letter{color:green;font-size:36px}`:

| разметка | первый `DrawText` (36 px) | ожидается |
|---|---|---|
| `<div>&#x0F3D;Test</div>` | `"༽"` | `"༽T"` |
| `<div>&#x2769;Test</div>` | `"❩"` | `"❩T"` |

## Как найдено

WPT-RUN-14 срез 15: `selectors/first-letter-punctuation-*` — в 321 из 339 id ведущий знак не ASCII. После снятия завершающей пунктуации (BUG-1373) и правки «:» (BUG-1366) у них остаются `thick`: 313 из 321.

## Что делать

Заменить предикат на проверку общей категории Unicode (`Ps`, `Pe`, `Pi`, `Pf`, `Po`). В workspace уже есть ICU4x (CAPABILITIES.md §lumen-encoding: «ICU4x 2.2 unicode provider»); `icu_properties::props::GeneralCategory` — без новой зависимости, если крейт уже подключён к `lumen-layout`, иначе новая зависимость требует блока «Why this dependency» (ADR-027).

## Как проверить

`css/CSS2/selectors/first-letter-punctuation-100.xht`…`-339.xht` после BUG-1366 и BUG-1373.
