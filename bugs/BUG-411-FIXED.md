# BUG-411 — движковый `#permPopover` не переносит домен/статус щитов и две строки разрешений

**Статус:** FIXED 2026-08-20 (P3)
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

## Исправление (2026-08-20, P3)

Сделаны все три пункта, плюс вскрывшийся по дороге четвёртый дефект.

**1. Дизайн-эталон и ассет.** Правка идёт в `docs/design/lumen-v3_3.html` (правило 4
`docs/tasks/p1-css-chrome.md`: ассет — производная), затем
`python scripts/gen_chrome_assets.py`. В `#permPopover` добавлены:

* строка щитов `#shieldSiteRow` — заголовок «Щиты на этом сайте», состояние
  `#shieldSiteState` (Вкл/Выкл) и переключатель `#shieldSiteToggle` с новым
  `data-action="toggle-site-shields"` (маппинг `toggleSiteShields(this)` →
  `ONCLICK_EXACT_ACTIONS`, отсюда `ChromeAction::ToggleSiteShields` в кодогенерации);
* `<span id="permDomain">` в заголовке «Разрешения — …»;
* две недостающие `perm-row` (Уведомления, Буфер обмена) + `<symbol id="i-bell">`.

Новой CSS нет — строка щитов переиспользует уже существующие `.set-row`/`.st-text`/
`.st-title`/`.st-desc`/`.toggle`/`.thumb`, поэтому парс-гейт `build.rs` не трогался.
Индексного маппинга это не сломало: `chrome_permission_kind_for_node` и так резолвит
позицию `.perm-row` среди детей поповера, так что индексы 2/3 стали достижимы сами
собой; строка щитов намеренно НЕ несёт класс `perm-row`.

**2. Модель.** `ChromeModel::permissions` расширен до `[ChromePermState; 4]`
(`chrome_model_snapshot` теперь мапит весь `PermissionKind::ALL`, а не первые два),
добавлены `popover_domain: String` и `site_shields_on: bool`. `bind_popover` принимает
`&ChromeModel` целиком и пишет домен (пустой → `NO_DOMAIN_LABEL` «этой странице», чтобы
демо-текст `figma.com` не выдавал себя за реальный хост), класс `.on` на переключателе и
текст состояния.

**3. Переключатель заведён на настоящий стейт, а не на индикатор.** Как и предлагала
оговорка ниже. `ShieldsPanel::enabled` (write-only после CC-15-4) заменён на пару
`default_enabled` + `site_overrides: HashMap<String, bool>`; читается через
`enabled_for_current()` (override хоста → иначе дефолт), флипается через
`toggle_current_site()` (без хоста — `about:blank`, `file://` — флип уходит в дефолт,
чтобы переключатель не был молчаливым no-op). Новый `Lumen::sync_adblock_filter()`
зеркалит это значение в `lumen_network::set_global_adblock_enabled`, то есть в реальный
гейт фильтра, и зовётся из пяти мест: старт, навигация (после `set_domain`), поповер,
настроечный `toggle-shields`, `close_settings_panel`. Per-site исключения живут сессию —
`BrowserSettings` хранит один плоский `shields_enabled`, отдельного стора под исключения
нет, заводить его в рамках багфикса не стали.

**4. Побочная находка — фильтрация выключалась на каждом переключении вкладки.**
`switch_tab` пушил в глобальный тумблер `TabEntry::adblock` — поле, которое писал
удалённый в CC-15 внутривкладочный чекбокс, то есть навсегда `false`. Вместе с тем, что
`config::init_adblock` стартует с `set_global_adblock_enabled(false)`, а персистнутую
настройку `BrowserSettings::shields_enabled` не читал никто, это означало: **включить
фильтрацию из UI было нельзя вообще**, а любое переключение вкладки гасило её до конца
сессии. Теперь `switch_tab` зовёт тот же `sync_adblock_filter()`, а стартовый код
подтягивает настройку. `TabEntry::adblock` оставлен (его гоняют session-снапшоты), но
помечен в доккоменте как vestigial — удаление это уборка дорожки CC, не багфикс.

**Тесты.** `lumen-chrome`: `popover_binds_blocked_total_and_permission_rows` расширен на
четыре строки, новый `popover_binds_domain_and_site_shields_row`. `lumen-shell`: четыре
новых юнит-теста per-site-логики в `shields_panel` (fallback на дефолт, изоляция
исключения по хостам, приоритет исключения над сменой дефолта, флип без хоста).

## Связанные

* [BUG-408](bugs/BUG-408-OPEN.md), [BUG-409](bugs/BUG-409-OPEN.md), [BUG-410](bugs/BUG-410-OPEN.md) —
  тот же класс: фичи легаси-хрома, не перенесённые в движковый и вскрытые срезами CC-15.
* [BUG-404](bugs/BUG-404-OPEN.md) — тот же поповер с другой стороны: его легаси-хит-тест жив и
  негейтирован, съедая клики по старому прямоугольнику.
