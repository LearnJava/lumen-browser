# BUG-421 — движковый `#view-settings` не изменяет ни одной настройки

**Статус:** FIXED 2026-08-01
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

## Закрыт 2026-08-01 (P1)

Из шести `.toggle` в `#view-settings` реально подключены два — Privacy →
«Adblock & Fingerprinting» → «Блокировать рекламу» и «Блокировать
фингерпринтинг», единственные с чистым 1:1 соответствием реальному
backing-полю (`ChromeSettingsModel::{ad_block_on,fingerprint_on}` уже
рефлектили их read-only с CC-10b):

1. Эталон (`docs/design/lumen-v3_3.html`, регенерируется
   `scripts/gen_chrome_assets.py`): эти два `.toggle` получили собственные
   `onclick="toggleShields(this)"`/`onclick="toggleFingerprintMode(this)"`
   вместо общего `toggleSwitch(this)` → `data-action="toggle-shields"`/
   `"toggle-fingerprint-mode"`. Две записи добавлены в
   `ONCLICK_EXACT_ACTIONS` генератора; `ChromeAction::{ToggleShields,
   ToggleFingerprintMode}` — автовывод `build.rs` из новых `data-action`.
   Отдельные действия вместо общего резолвера (`data-setting` + аналог
   `chrome_permission_kind_for_node`) — для двух конкретных, структурно
   неотличимых друг от друга контролов собственный `data-action` проще и
   ничем не хуже.
2. `SettingsPanel` (`panels/settings_panel.rs`) получил методы
   `toggle_shields`/`toggle_fingerprint_mode`, мутирующие `draft` (по образцу
   уже существующих `append_char`/`backspace`) — юнит-тестируемы без полного
   `Lumen`-фикстура, в отличие от инлайн-мутации прямо в
   `dispatch_chrome_action`. `fingerprint_mode` — трёхзначная строка
   (`"standard"`/`"strict"`/`"off"`), но тумблер экспонирует только вкл/выкл
   (то же упрощение, что уже делал read-side `fingerprint_on`) — включение
   всегда приземляется на дефолтный `"standard"`, не восстанавливая прежний
   `"strict"`.
3. `dispatch_chrome_action` обрабатывает оба новых действия вызовом этих
   методов + `relayout_chrome_host()`; персистятся штатным путём — на закрытии
   панели `close_settings_panel` → `settings_store.apply_snapshot(&draft)`,
   тем же механизмом, что и остальные поля `draft`.

**Сознательно не тронуто** (тот же honesty-over-fabrication класс, что и в
[BUG-420](BUG-420-FIXED.md)/`ChromeSettingsModel`/`#statAds`): «Принудительный
HTTPS» (нет соответствующего поля вовсе), два `.toggle` в разделе
Extensions и один в QA (нет стора расширений/QA-флагов), Shields-радио
(Standard/Strict/Tor-like — не 1:1 с одиночным `shields_enabled: bool`),
радио-карточки General/Appearance/Sync, таблица Permissions — статика
эталона без бэкинга. Разделы `general`/`appearance`/`sync`/`ext`/`qa` в
дизайне вообще не покрывают то, что описывает doc-comment
`SettingsPanel` (Downloads/Network/Adblock/Language, homepage/download-path
текстовые поля, выбор темы/акцента) — этой разметки в замороженном v3.3
эталоне (DS-1…DS-19 complete) попросту нет; добавление новых секций/полей —
дизайн-система-работа, а не баг-фикс, и не предпринималось.
`SettingInput::{Homepage,DownloadPath}`, `ThemeChoice::to_settings_str`,
`AccentPreset::{ALL,key}` остаются под `#[allow(dead_code)]` — честно, ничего
их не читает.

## Связанные

* [BUG-420](BUG-420-FIXED.md) — тот же класс, уже закрыт (`#printOverlay`);
  тот же приём (различающий `data-action` вместо общего резолвера).
* [BUG-411](BUG-411-FIXED.md) — тот же класс в `#permPopover` (недостижимые
  строки разрешений), ещё открыт.
* CC-9/CC-10/CC-10b (`docs/tasks/p1-css-chrome.md`) — срезы, где
  `#view-settings` получил отображение, но не запись.
