# BUG-409 — группы вкладок не отображаются в движковом хроме

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/tabs/groups.rs`, `crates/shell/src/tabs/strip.rs`),
chrome (`crates/chrome/src/model.rs::ChromeTabModel`, `assets/chrome/chrome.html`)
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

## Связанные

* [BUG-408](bugs/BUG-408-OPEN.md), [BUG-410](bugs/BUG-410-OPEN.md) — соседние непереносённые
  куски того же легаси-таб-бара/омнибокса, найдены тем же срезом.
* CC-8 (`docs/tasks/p1-css-chrome.md`) — срез, где `ChromeTabModel` получил `container_color`
  и `is_child`, но не групповые поля.
* CC-15-3 (`ROADMAP.md`) — срез, вскрывший пробел.
