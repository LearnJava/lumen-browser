# BUG-1439 — `CSSStyleSheet`, оторванный от документа (`<style>` снят через `removeChild`), теряет правила: `insertRule` бросает `IndexSizeError`

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:12069-12086` — `_lumen_make_css_style_sheet`, `cur()`)

## Симптом

`const {sheet} = style` у `<style>`, добавленного и снятого из `<head>`: `sheet.insertRule('a{}')` бросает `IndexSizeError: … the index provided is larger than the maximum index`, `deleteRule(0)` бросает то же, `cssRules.length` остаётся 0. У прикреплённого листа те же вызовы работают. `cur()` (BUG-493) возвращает `-1`, как только владелец вышел из дерева, и обе функции трактуют `-1` как «индекс вне диапазона». Так устроен общий помощник WPT `/css/support/parsing-testcommon.js` (`test_valid_selector`, `test_invalid_selector`, `test_valid_rule`, `test_invalid_rule`): 59 id этого среза, 358 упавших сабтестов с этим сообщением (`parse-not`, `parse-has`, `parse-is-where`, `parse-state`, `parse-heading`, `css-cascade/parsing/layer.html`, `css-pseudo/parsing/highlight-pseudos.html`, `focus-visible-017-2` и др.); 77 файлов `css/` вызывают семейство `test_*_selector/rule`, 53 из них в `css/selectors`. Остальные проверки этих файлов (разбор, сериализация) за этим сообщением не видны — после правки часть станет другими падениями.

## Проба

| вызов | прикреплённый лист | лист после `document.head.removeChild(style)` |
|---|---|---|
| `sheet.insertRule("a{}")` | `1 a` | **`IndexSizeError`** (`the index provided is larger than the maximum index`) |
| `sheet.insertRule("a{}", 0)` | работает | **`IndexSizeError`** |
| `sheet.deleteRule(0)` после успешной вставки | работает | **`IndexSizeError`** |
| `sheet.cssRules.length` | 0 → 1 после вставки | 0 |
| `typeof sheet.insertRule` | `function` | `function` |

Проба (`--mcp`, `eval`): `const style=document.createElement("style");document.head.append(style);const {sheet}=style;document.head.removeChild(style);const {cssRules}=sheet;sheet.insertRule("a{}")`.

## Как найдено

WPT-RUN-14 срез 20: 358 сабтестов `Failed to execute 'insertRule' on 'CSSStyleSheet': the index provided is larger than the maximum index` в 59 id; причина установлена пробой выше.

## Что делать

По CSSOM §6.2 `CSSStyleSheet` — самостоятельный объект: после снятия `<style>` он уходит из `document.styleSheets`, но правила остаются в нём и доступны (`insertRule`/`deleteRule`/`cssRules`). Для листа без владельца в дереве нужен собственный реестр правил (как у `new CSSStyleSheet()`, `_lumen_make_constructed_style_sheet`), а не `-1`. Не ломать BUG-493: `tag.sheet` после `replaceChildren`/замены текста должен по-прежнему отражать новый разбор.

## Как проверить

Проба из таблицы; `css/selectors/parsing/parse-not.html`, `css/selectors/focus-visible-017-2.html`, `css/css-cascade/parsing/layer.html`.

## Повторное измерение: WPT-RUN-14 срез 22 (2026-10-08)

`css-view-transitions/parsing/pseudo-elements-{valid,valid-with-classes,invalid,invalid-with-classes}.html` — 100 + 164 + 675 + 20 сабтестов с этой же причиной (`insertRule` на листе отсоединённого `<style>` — `IndexSizeError`; пробы `.tmp/s22/p10.py`, `p11.py`): тест ждёт `SyntaxError` на невалидный селектор, получает `IndexSizeError`.

## Срез 25 (2026-10-08, P2, WPT-RUN-14 `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)

`css/cssom/CSSStyleSheet-modify-after-removal.html` (1 id): `sh = style.sheet; head.removeChild(style); sh.cssRules.length` — `0`, `style.sheet` — `null` (ожидается: таблица остаётся доступной для `insertRule` и `cssRules`).

## Исправление (2026-10-21)

- `SheetSync::detached` (`sheet_sync.rs`): сверка реестра с DOM откладывает лист каждого узла, вышедшего из дерева (до 256 штук, старые вытесняются), вместо того чтобы выбросить.
- Натив `_lumen_stylesheet_take_detached(nid)` (`cssom_tree_natives.rs`) переселяет отложенный лист в реестр конструируемых и возвращает его индекс.
- Шим: `be.idx()` владельческого бэкенда при `-1` вызывает `_lumen_css_detach` — бэкенд переключается на `(constructed, idx)`, помечается `detached` (`ownerNode === null`, `replaceSync` по-прежнему `NotAllowedError`, `@import` в `insertRule` разрешён), убирается из карты владельцев — повторная вставка того же `<style>` даёт свежий лист, старый объект остаётся оторванным. CSSOM-правки оторванного листа в журнал каскада не пишутся.
- Тесты: `v8_bug493_sheet_registry` (`sheet_of_removed_style_keeps_its_rules`, `removed_sheet_reports_syntax_error_and_stays_non_constructed`, `reinserted_style_gets_a_fresh_sheet`).

Не затронуто: `CSSStyleSheet-modify-after-removal.html` — там лист снимается вместе с iframe, другой механизм.
