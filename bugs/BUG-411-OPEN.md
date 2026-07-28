# BUG-411 — движковый `#permPopover` не переносит домен/статус щитов и две строки разрешений

**Статус:** OPEN
**Компонент:** shell (`Lumen::chrome_model_snapshot`, `crates/shell/src/panels/shields_panel.rs`,
`panels/permission_panel.rs`), chrome (`ChromeModel::blocked_total`/`permissions`,
`assets/chrome/chrome.html` `#permPopover`)
**Найден:** P1, CC-15-4 (2026-07-28), при удалении легаси-покраски поповера

## Симптом

Легаси-панель щитов (`shields_panel::build_panel`, удалена в CC-15-4) показывала четыре вещи:
домен текущей страницы, статус «SHIELDS ON/OFF», счётчик заблокированных запросов и полосу-переключатель
«Disable/Enable for this site». Легаси-панель разрешений (`permission_panel::build_panel`, удалена там же)
рисовала **все четыре** `PermissionKind::ALL` — Camera, Microphone, Notifications, Clipboard.

Движковый `#permPopover` (CC-9, `bind_popover`) переносит из этого только счётчик (`#statTrackers`)
и первые две строки разрешений:

```rust
// main.rs, chrome_model_snapshot
let permissions = [PermissionKind::Camera, PermissionKind::Microphone]
    .map(|kind| /* … */);
```

Итог под дефолтным хромом (с CC-14):

* домен, к которому относится поповер, нигде не показан;
* статуса щитов и переключателя «для этого сайта» нет — `ShieldsPanel::enabled` после CC-15-4
  пишется живым (негейтированным) хит-тестом, но не читается ничем; `current_domain` пишется
  `set_domain()` на каждую навигацию и тоже не читается;
* разрешения Notifications и Clipboard недоступны из UI: `chrome_permission_kind_for_node`
  отображает индекс строки хрома в `PermissionKind::ALL`, а строк в `chrome.html` только две,
  так что индексы 2/3 недостижимы. Состояние этих двух разрешений живёт в `PermissionPanel`,
  меняется только через удалённую легаси-панель.

Ограничение по двум строкам зафиксировано в доккоменте `ChromeModel::permissions` как осознанное
(«The design has no rows for `Notifications`/`Clipboard`»), но баг на потерю функции заведён не был.

## Оговорка про переключатель щитов

`ShieldsPanel::enabled` и в легаси-режиме был **только индикатором**: единственный его потребитель —
покраска панели, реальный ад-блок переключается независимым `SettingsPanel::draft.shields_enabled`
(`#view-settings`). То есть по функции потеряны домен + индикация + недостижимые строки разрешений,
а не работающее отключение фильтрации для сайта. Если поповер восстанавливать — переключатель имеет
смысл сразу завести на настоящий per-site-стейт, а не воспроизводить индикатор.

## Что нужно сделать

1. Добавить в `ChromeModel` поля для домена и статуса щитов, привязать к элементам `#permPopover`
   (потребуется правка `assets/chrome/chrome.html` — в дизайне этих элементов нет).
2. Либо добавить в асcет две недостающие строки `perm-row` (Notifications, Clipboard) и расширить
   `ChromeModel::permissions` до `[ChromePermState; 4]`, либо явно признать их вне охвата и удалить
   соответствующие варианты `PermissionKind` вместе с их состоянием.
3. Решить судьбу `ShieldsPanel::enabled`/`current_domain` (см. оговорку выше).

## Связанные

* [BUG-408](bugs/BUG-408-OPEN.md), [BUG-409](bugs/BUG-409-OPEN.md), [BUG-410](bugs/BUG-410-OPEN.md) —
  тот же класс: фичи легаси-хрома, не перенесённые в движковый и вскрытые срезами CC-15.
* [BUG-404](bugs/BUG-404-OPEN.md) — тот же поповер с другой стороны: его легаси-хит-тест жив и
  негейтирован, съедая клики по старому прямоугольнику.
