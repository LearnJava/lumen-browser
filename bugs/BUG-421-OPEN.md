# BUG-421 — движковый `#view-settings` не изменяет ни одной настройки

**Статус:** OPEN
**Компонент:** chrome (`assets/chrome/chrome.html` `#view-settings`,
`ChromeSettingsModel`, `ChromeAction::ToggleSwitch`), shell
(`crates/shell/src/panels/settings_panel.rs`, `panels/themes.rs`,
`dispatch_chrome_action`)
**Найден:** P1, CC-15-6 (2026-07-28), при удалении легаси-хит-теста настроек

## Симптом

Движковый `#view-settings` (CC-9) отображает состояние (секции переключаются
`ChromeAction::SetSettingsSection`, `bind_settings` рефлектит две настройки с
реальным backing-состоянием), но **записать** ничего не может:

* `ChromeAction::ToggleSwitch` — no-op с самого CC-9: все шесть `.toggle` в
  эталоне несут один и тот же `data-action` без различающего атрибута, резолвер
  (аналог `chrome_permission_kind_for_node`) не написан;
* выбор темы/акцента, поле homepage, путь загрузок, сброс раскладки панелей,
  переключатель HTTP/3, включение/выключение подписок ад-блока и ручной refresh
  — в ассете отдельных действий нет вовсе.

Следствия, вскрытые удалением легаси-хит-теста (`settings_panel::hit_test` и
`ht_*` — единственные писатели этих полей):

* `SettingsPanel::draft` больше **никто не изменяет** — `open_settings_panel`
  заполняет черновик из стора, `close_settings_panel` пишет его обратно
  неизменным. Тема (`draft.theme` → `ShellTheme::parse`), раскладка вкладок,
  DoH/щиты, размер шрифта — все read-only;
* `SettingInput::{Homepage, DownloadPath}` никогда не конструируются →
  `focused_input` всегда `None` → клавиатурное редактирование текстовых полей
  (`append_char`/`backspace`) мертво;
* `panels::themes::ThemeChoice::to_settings_str` и `AccentPreset::{ALL, key}`
  остались без читателей (были нужны только сериализации выбора темы).

Всё перечисленное помечено `#[allow(dead_code, reason = "BUG-421: …")]` —
удалять до реализации нельзя, это и есть точки подключения.

Регрессия флипа CC-14 (не CC-15-6): легаси-панель настроек перестала
рисоваться уже тогда, CC-15-4 удалила её покраску, CC-15-6 — хит-тест.

## Что нужно сделать

1. Разметить контролы `#view-settings` различающими атрибутами
   (`data-setting="homepage"`/`"shields"`/`"doh"`/`"http3"`/`"theme"`/…),
   регенерировать ассет через `scripts/gen_chrome_assets.py`.
2. Резолвер `chrome_setting_for_node` по образцу
   `chrome_permission_kind_for_node`, и обработка `ToggleSwitch`/новых действий
   в `dispatch_chrome_action` с записью в `SettingsPanel::draft`.
3. Расширить `ChromeSettingsModel` до полного снапшота и связать в
   `bind_settings`.
4. Снять `#[allow(dead_code)]` с `SettingInput`, `ThemeChoice::to_settings_str`,
   `AccentPreset::{ALL, key}`.

## Связанные

* [BUG-420](BUG-420-OPEN.md) — тот же пробел в `#printOverlay`.
* [BUG-411](BUG-411-OPEN.md) — тот же класс в `#permPopover` (недостижимые
  строки разрешений).
* CC-9/CC-10 (`docs/tasks/p1-css-chrome.md`) — срезы, где `#view-settings`
  получил отображение, но не запись.
