# BUG-409 — группы вкладок не отображаются в движковом хроме

**Статус:** FIXED 2026-07-31 (P1)
**Компонент:** shell (`crates/shell/src/tabs/groups.rs`, `crates/shell/src/tabs/strip.rs`,
`crates/shell/src/main.rs::chrome_model_snapshot`),
chrome (`crates/chrome/src/model.rs::ChromeTabModel`, `assets/chrome/chrome.html`,
`docs/design/lumen-v3_3.html`)
**Найден:** P1, CC-15-3 (2026-07-28), при вырезании легаси-покраски таб-бара

## Симптом

Модель групп вкладок жива и полностью функциональна: `TabGroup { name, color, collapsed }`,
`TabStrip::{group_of, is_collapsed, group_members, group_color}`, назначение цвета по
`GroupColor::from_index` при создании группы (`main.rs`), команды группировки в контекстном
меню вкладки. Единственным потребителем *визуальной* части был легаси-покрасчик таб-бара
(`strip::build_tab_bar` рисовал цветную полоску-акцент над каждой вкладкой группы и
подпись группы в её цвете).

`ChromeTabModel` (CC-8) переносит в движковый хром `id`/`title`/`active`/`sleeping`/
`is_child`/`container_color`, но **не** несёт ни цвет группы, ни имя группы, ни признак
свёрнутости. `assets/chrome/chrome.html` соответствующего элемента не содержит. С флипа
дефолта (CC-14) группы вкладок стали невидимы: вкладки в группе выглядят как обычные,
свёрнутая группа не отличается от развёрнутой, цвет группы нигде не показан.

CC-15-3 удалила уже мёртвый легаси-покрасчик, из-за чего `GroupColor::color()` и
`TabStrip::group_color()` остались без читателей (помечены
`#[allow(dead_code, reason = "BUG-409: …")]` — палитра сохранена как источник истины
для переноса, а не удалена).

## Что нужно сделать

Расширить `ChromeTabModel` полем вида `group: Option<ChromeTabGroup { color: String, name: String, collapsed: bool }>`
(цвет — `#RRGGBB` из `GroupColor::color()`, тем же способом, что уже сделан
`container_color`), добавить в `assets/chrome/chrome.html` элемент акцента/подписи группы
и связать в `bind_model`. После этого снять оба `#[allow(dead_code)]`.

Свёрнутая группа дополнительно требует решения, скрывать ли её вкладки в списке
`rebuild_tab_list` (легаси-полоса их не рисовала) — это меняет `ChromeMutations`-диффы
списка, так что стоит проверить перф-гейт CC-12 после изменения.

## Фикс (2026-07-31, P1)

Полный перенос, по образцу `container_color` (CC-8).

1. **Эталон** (`docs/design/lumen-v3_3.html`): новый CSS-класс `.group-stripe`
   (идентичен по форме `.container-stripe` — та же 3px полоска, свой класс, чтобы
   контейнер и группа могли рисоваться на одной строке одновременно, это
   ортогональные концепции) и одна демо-строка `.tab-row` с `.group-stripe` в
   примере сайдбара, для CSS-гейта. Перегенерировано
   (`python scripts/gen_chrome_assets.py`) — правило «изменения только через
   реф» (CC-13) соблюдено, `assets/chrome/chrome.html` руками не редактировался.
2. **`crates/chrome/src/model.rs`**: новый `ChromeTabGroup { color, name, collapsed }`
   + поле `ChromeTabModel::group: Option<ChromeTabGroup>`. `populate_tab_row_children`
   рисует `.group-stripe` перед `.container-stripe` (обе полоски могут стоять рядом),
   с `title` = имя группы (сайдбар слишком узкий для полноразмерной подписи текстом —
   тот же приём, что `.tab-badge`'s `title="Гибернирована"`); для свёрнутой группы
   тултип получает суффикс «(свёрнута)» (`group_stripe_title`). `update_tab_row`
   получил тот же shape-mismatch-fallback, что уже был у `container_color`
   (появление/исчезновение `.group-stripe` → перестройка детей строки, `NodeId`
   строки сохраняется). 4 новых юнит-теста.
3. **`crates/shell/src/main.rs::chrome_model_snapshot`**: список вкладок строится по
   `tab_strip.visible_indices()`, а не по `tab_strip.tabs` напрямую — не-крайние
   участники свёрнутой группы скрыты за крайним (chip), как это делала легаси-полоса
   (`strip::build_tab_bar` тоже проходила по `visible_indices()`). Клики по-прежнему
   резолвятся по `data-tab-id` = стабильный id вкладки, а не по позиции в списке, так
   что фильтрация безопасна — существующий контекстно-меню путь
   (`MenuAction::ToggleGroupCollapse`) остаётся единственным способом развернуть
   группу обратно, он не завязан на геометрию хрома. `group` строится из
   `TabStrip::{group, group_color}` (оба расчехлены — сняты `#[allow(dead_code)]` с
   `GroupColor::color()`/`TabStrip::group_color()`, как просил план фикса).

Не сделано намеренно: отдельного визуального индикатора «эта строка — свёрнутая
группа» (кроме тултипа) — новая иконка/бейдж потребовала бы либо третьего варианта
хвостового слота в `update_tab_row` (сейчас бинарный badge/close), либо отдельного
ряда-заголовка группы; ни один из вариантов не входил в формулировку «элемент
акцента/подписи», а хвостовой слот усложнил бы shape-matching без явного запроса.
Разворот свёрнутой группы остаётся только через контекстное меню (уже работал
независимо от этого фикса).

Верификация: `cargo test -p lumen-chrome` (68/68), `cargo test -p lumen-shell tabs::`
(153/153, включая уже существовавшие `visible_indices_*`/`group_color`-тесты),
`cargo clippy -p lumen-chrome --all-targets -- -D warnings`,
`cargo clippy -p lumen-shell --all-targets -- -D warnings`,
`python scripts/gen_chrome_assets.py --check`, `python graphic_tests/run.py --continue-on-fail`
(новая разметка хрома может двигать пиксели, тот же риск что BUG-408).

## Связанные

* [BUG-408](BUG-408-FIXED.md), [BUG-410](BUG-410-FIXED.md) — соседние
  куски того же легаси-таб-бара/омнибокса, найдены тем же срезом; BUG-408 —
  прямой образец для этого фикса (тот же перенос-через-реф путь).
* CC-8 (`docs/tasks/p1-css-chrome.md`) — срез, где `ChromeTabModel` получил `container_color`
  и `is_child`, но не групповые поля.
* CC-15-3 (`ROADMAP.md`) — срез, вскрывший пробел.
* [BUG-404](BUG-404-FIXED.md) — остаётся: правый клик по вкладке (контекстное
  меню, единственный путь развернуть свёрнутую группу) хит-тестится легаси-геометрией.
