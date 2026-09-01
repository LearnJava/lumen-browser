//! The shell's keyboard layer: which action a physical key plus modifiers
//! means, and where the focused text control keeps the value a keystroke
//! edits.
//!
//! [`keybinding_for`] is deliberately isolated from winit's event loop so the
//! mapping is testable without one; [`TypeableField`] is the other half of the
//! same path — `Lumen::inject_char` asks `typeable_field` which storage model
//! the focused control uses before `edit_focused_field` writes the new value
//! back (BUG-436).
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3d); behaviour and
//! signatures are unchanged.

use crate::*;

/// Where a mutable form control keeps the text it renders — the two storage
/// models HTML gives text-editing controls (BUG-436).
///
/// Picked by [`Lumen::typeable_field`] and consumed by
/// [`Lumen::edit_focused_field`], which has to write the new value back to the
/// right place: `<input>` reflects it in the `value` content attribute,
/// `<textarea>` in its text-node children (HTML LS §4.10.11).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum TypeableField {
    /// `<input>` of a text-like type — value lives in the `value` attribute.
    Input,
    /// `<textarea>` — value lives in the element's text-node children.
    Textarea,
}

/// Действия shell-а, на которые мапятся клавиши. Изолированы от winit, чтобы
/// маппер был тестируем без event loop.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum KeyCommand {
    Reload,
    Exit,
    FindOpen,
    /// Открыть адресную строку (Ctrl+L / F6). Позволяет ввести URL или
    /// поисковый запрос прямо в браузере без перезапуска из CLI.
    OpenAddressBar,
    /// Навигация назад (Alt+Left). Восстанавливает из bfcache если возможно.
    HistoryBack,
    /// Навигация вперёд (Alt+Right). Восстанавливает из bfcache если возможно.
    HistoryForward,
    /// Скролл на одну строку вниз (стрелка вниз).
    ScrollLineDown,
    /// Скролл на одну строку вверх (стрелка вниз).
    ScrollLineUp,
    /// Скролл на ~90% viewport-а вниз (PageDown / Space).
    ScrollPageDown,
    /// Скролл на ~90% viewport-а вверх (PageUp / Shift+Space).
    ScrollPageUp,
    /// Прыжок к началу документа (Home).
    ScrollHome,
    /// Прыжок к концу документа (End).
    ScrollEnd,
    /// Горизонтальный скролл на одну колонку вправо (стрелка вправо).
    ScrollLineRight,
    /// Горизонтальный скролл на одну колонку влево (стрелка влево).
    ScrollLineLeft,
    /// Открыть hint-режим: показать kbd-бейджи на всех кликабельных элементах (F).
    HintModeOpen,
    /// Открыть новую вкладку (Ctrl+T).
    NewTab,
    /// Закрыть текущую вкладку или выйти, если вкладка последняя (Ctrl+W).
    CloseTab,
    /// Переключиться на следующую вкладку циклически (Ctrl+Tab).
    NextTab,
    /// Открыть/закрыть панель загрузок (Ctrl+Shift+J).
    DownloadsPanel,
    /// Открыть/закрыть split view (Ctrl+\): показывает активную и следующую
    /// вкладку рядом. При повторном нажатии закрывает split.
    SplitView,
    /// Переключить фокус между левой и правой панелями split view (Ctrl+M).
    SplitFocusSwitch,
    /// Включить/выключить Vim-режим навигации (Ctrl+Alt+V).
    VimModeToggle,
    /// Показать/скрыть вертикальную панель вкладок (Ctrl+B).
    ToggleVerticalTabs,
    /// Показать/скрыть tree-style панель вкладок (Ctrl+Shift+B).
    ToggleTreeTabs,
    /// Перенести активный сайдбар вкладок к противоположному краю окна (Ctrl+Alt+B).
    FlipActiveDock,
    /// Показать/скрыть панель воркспейсов (Ctrl+Shift+W).
    ToggleWorkspaces,
    /// Показать/скрыть панель Shields (Ctrl+Shift+S).
    ToggleShields,
    /// Показать/скрыть панель разрешений сайта (Ctrl+Shift+P, 7C.2).
    TogglePermissions,
    /// Включить/выключить авто-закрытие cookie-баннеров (Ctrl+Shift+K, 7C.3).
    ToggleCookieBannerDismiss,
    /// Показать/скрыть AI-панель (Ctrl+Shift+A, §12.8).
    ToggleAiPanel,
    /// Открыть/закрыть панель настроек доступности (Ctrl+Shift+Q, E-2).
    ToggleA11y,
    /// Показать/скрыть менеджер закладок (Ctrl+Shift+O, task #22).
    ToggleBookmarks,
    /// Показать/скрыть панель истории браузера (Ctrl+H, task D-5).
    ToggleHistory,
    /// Открыть/закрыть страницу настроек браузера (Ctrl+,, task D-7).
    ToggleSettings,
    /// Показать/скрыть командную палитру (Ctrl+K, §7E.2, task #23).
    ToggleCommandPalette,
    /// Войти/выйти из focus mode + Pomodoro (Ctrl+Shift+F, task #25, V4).
    ToggleFocusMode,
    /// Добавить текущую страницу в закладки (Ctrl+D).
    BookmarkCurrentPage,
    /// Показать/скрыть DevTools JS-консоль (F12, §7E.5).
    DevConsole,
    /// Показать/скрыть DevTools DOM-инспектор (Ctrl+Shift+I, §7E.1).
    DevInspector,
    /// Показать/скрыть DevTools панель сети (Ctrl+Shift+E, §7E.4).
    DevNetwork,
    /// Показать/скрыть privacy-панель сети (Ctrl+Shift+Y, V5).
    TogglePrivacy,
    /// Открыть/закрыть picture-in-picture окно видео (Ctrl+Shift+V, task #21).
    TogglePip,
    /// Показать/скрыть панель Read-later (Ctrl+Shift+R, §12.3).
    ToggleReadLater,
    /// Включить/выключить Reader View (F9, §D-3): clean article layout.
    ToggleReaderView,
    /// Открыть просмотр исходного кода текущей страницы (Ctrl+U, §D-2).
    ViewSource,
    /// Открыть/закрыть панель горячих клавиш (Ctrl+Shift+/, §D-4).
    ToggleShortcuts,
    /// Открыть/закрыть диалог печати страницы (Ctrl+P, E-1).
    TogglePrint,
    /// Открыть/закрыть просмотр TLS-сертификата (Ctrl+Shift+C, §D-1).
    ToggleCert,
    /// Назначить контейнер активной вкладке (7D.2). Не привязано к клавише —
    /// диспатчится программно (контекстное меню вкладки / omnibox-команда
    /// `container <name>`). См. `tabs::containers::ContainerKind`.
    ///
    /// Конструируется через шелл-команды/omnibox в follow-up таске; пока
    /// гасим dead_code-предупреждение, чтобы `cargo clippy -D warnings` прошёл.
    #[allow(dead_code)]
    SetTabContainer(tabs::containers::ContainerKind),
    /// Увеличить масштаб страницы (Ctrl+=).
    ZoomIn,
    /// Уменьшить масштаб страницы (Ctrl+-).
    ZoomOut,
    /// Сбросить масштаб страницы к 100% (Ctrl+0).
    ZoomReset,
}

/// Маппинг физической клавиши + модификаторов на shell-action.
///
/// F5 без модификаторов  → Reload.
/// Ctrl+R                → Reload.
/// Esc без модификаторов → Exit.
/// Ctrl+W                → Exit.
/// Ctrl+F                → FindOpen.
/// F (без модификаторов) → HintModeOpen (kbd-навигация по ссылкам/кнопкам).
/// ↓ / ↑                 → ScrollLineDown / ScrollLineUp (без модификаторов).
/// → / ←                 → ScrollLineRight / ScrollLineLeft (без модификаторов).
/// PageDown / PageUp     → ScrollPageDown / ScrollPageUp.
/// Space / Shift+Space   → ScrollPageDown / ScrollPageUp (привычка пробела в браузерах).
/// Home / End            → ScrollHome / ScrollEnd.
///
/// Прочие комбинации (Ctrl+Shift+R, F5+Ctrl, и т.д.) — пока None: не хотим
/// перехватывать привычные web-shortcuts (force-reload, etc.) до того, как
/// решим, что они должны делать.
pub(crate) fn keybinding_for(code: KeyCode, mods: ModifiersState) -> Option<KeyCommand> {
    let ctrl_only = mods == ModifiersState::CONTROL;
    let shift_only = mods == ModifiersState::SHIFT;
    let alt_only = mods == ModifiersState::ALT;
    let no_mods = mods.is_empty();
    let ctrl_and_shift = mods == (ModifiersState::CONTROL | ModifiersState::SHIFT);
    match code {
        KeyCode::F5 if no_mods => Some(KeyCommand::Reload),
        KeyCode::KeyR if ctrl_only => Some(KeyCommand::Reload),
        KeyCode::Escape if no_mods => Some(KeyCommand::Exit),
        KeyCode::KeyW if ctrl_only => Some(KeyCommand::CloseTab),
        KeyCode::KeyT if ctrl_only => Some(KeyCommand::NewTab),
        KeyCode::Tab if ctrl_only => Some(KeyCommand::NextTab),
        KeyCode::KeyF if ctrl_only => Some(KeyCommand::FindOpen),
        KeyCode::KeyF if no_mods => Some(KeyCommand::HintModeOpen),
        KeyCode::KeyL if ctrl_only => Some(KeyCommand::OpenAddressBar),
        KeyCode::F6 if no_mods => Some(KeyCommand::OpenAddressBar),
        KeyCode::ArrowLeft if alt_only => Some(KeyCommand::HistoryBack),
        KeyCode::ArrowRight if alt_only => Some(KeyCommand::HistoryForward),
        KeyCode::ArrowDown if no_mods => Some(KeyCommand::ScrollLineDown),
        KeyCode::ArrowUp if no_mods => Some(KeyCommand::ScrollLineUp),
        KeyCode::ArrowRight if no_mods => Some(KeyCommand::ScrollLineRight),
        KeyCode::ArrowLeft if no_mods => Some(KeyCommand::ScrollLineLeft),
        KeyCode::PageDown if no_mods => Some(KeyCommand::ScrollPageDown),
        KeyCode::PageUp if no_mods => Some(KeyCommand::ScrollPageUp),
        KeyCode::Space if no_mods => Some(KeyCommand::ScrollPageDown),
        KeyCode::Space if shift_only => Some(KeyCommand::ScrollPageUp),
        KeyCode::Home if no_mods => Some(KeyCommand::ScrollHome),
        KeyCode::End if no_mods => Some(KeyCommand::ScrollEnd),
        KeyCode::KeyJ if ctrl_only => Some(KeyCommand::DownloadsPanel),
        KeyCode::KeyJ if ctrl_and_shift => Some(KeyCommand::DownloadsPanel),
        // Ctrl+\ — toggle split view (show active + next tab side-by-side)
        KeyCode::Backslash if ctrl_only => Some(KeyCommand::SplitView),
        // Ctrl+M — move focus between left / right pane in split mode
        KeyCode::KeyM if ctrl_only => Some(KeyCommand::SplitFocusSwitch),
        // Ctrl+Alt+V — toggle Vim navigation mode
        KeyCode::KeyV if mods == (ModifiersState::CONTROL | ModifiersState::ALT) => {
            Some(KeyCommand::VimModeToggle)
        }
        // Ctrl+B — toggle vertical tab sidebar
        KeyCode::KeyB if ctrl_only => Some(KeyCommand::ToggleVerticalTabs),
        // Ctrl+Shift+B — toggle tree-style tab sidebar
        KeyCode::KeyB if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleTreeTabs)
        }
        // Ctrl+Alt+B — move the active tab sidebar to the opposite edge (cross-dock)
        KeyCode::KeyB if mods == (ModifiersState::CONTROL | ModifiersState::ALT) => {
            Some(KeyCommand::FlipActiveDock)
        }
        // Ctrl+Shift+W — toggle workspace switcher bar
        KeyCode::KeyW if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleWorkspaces)
        }
        // Ctrl+Shift+S — toggle shields panel
        KeyCode::KeyS if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleShields)
        }
        // Ctrl+Shift+P — toggle per-site permission popover (7C.2)
        KeyCode::KeyP if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePermissions)
        }
        // Ctrl+Shift+K — toggle cookie-banner auto-dismiss (7C.3)
        KeyCode::KeyK if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleCookieBannerDismiss)
        }
        // Ctrl+Shift+A — toggle AI sidebar panel (§12.8)
        KeyCode::KeyA if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleAiPanel)
        }
        // Ctrl+Shift+O — toggle bookmark manager panel (task #22)
        KeyCode::KeyO if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleBookmarks)
        }
        // Ctrl+H — toggle browser history panel (task D-5)
        KeyCode::KeyH if ctrl_only => Some(KeyCommand::ToggleHistory),
        // Ctrl+, — open browser settings (task D-7)
        KeyCode::Comma if ctrl_only => Some(KeyCommand::ToggleSettings),
        // Ctrl+Shift+F — toggle focus mode + Pomodoro timer (task #25, V4)
        KeyCode::KeyF if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleFocusMode)
        }
        // Ctrl+K — toggle the command palette (§7E.2)
        KeyCode::KeyK if ctrl_only => Some(KeyCommand::ToggleCommandPalette),
        // Ctrl+D — bookmark the current page
        KeyCode::KeyD if ctrl_only => Some(KeyCommand::BookmarkCurrentPage),
        // F12 — toggle DevTools JS console (§7E.5)
        KeyCode::F12 if no_mods => Some(KeyCommand::DevConsole),
        // Ctrl+Shift+I — toggle DevTools DOM inspector (§7E.1)
        KeyCode::KeyI if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::DevInspector)
        }
        // Ctrl+Shift+E — toggle DevTools network panel (§7E.4)
        KeyCode::KeyE if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::DevNetwork)
        }
        // Ctrl+Shift+Y — toggle privacy network panel (V5)
        KeyCode::KeyY if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePrivacy)
        }
        // Ctrl+Shift+V — toggle picture-in-picture video window (task #21)
        KeyCode::KeyV if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePip)
        }
        // Ctrl+Shift+Q — toggle accessibility settings panel (E-2)
        KeyCode::KeyQ if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleA11y)
        }
        // Ctrl+Shift+R — toggle Read-later panel (§12.3)
        KeyCode::KeyR if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleReadLater)
        }
        // F9 — toggle Reader View (§D-3)
        KeyCode::F9 if no_mods => Some(KeyCommand::ToggleReaderView),
        // Ctrl+U — view page source (§D-2)
        KeyCode::KeyU if ctrl_only => Some(KeyCommand::ViewSource),
        // Ctrl+Shift+/ — toggle keyboard shortcuts panel (§D-4)
        KeyCode::Slash if ctrl_and_shift => Some(KeyCommand::ToggleShortcuts),
        // Ctrl+P — print dialog (E-1)
        KeyCode::KeyP if ctrl_only => Some(KeyCommand::TogglePrint),
        // Ctrl+Shift+C — certificate viewer (§D-1)
        KeyCode::KeyC if ctrl_and_shift => Some(KeyCommand::ToggleCert),
        // Ctrl+= — zoom in
        KeyCode::Equal if ctrl_only => Some(KeyCommand::ZoomIn),
        // Ctrl+- — zoom out
        KeyCode::Minus if ctrl_only => Some(KeyCommand::ZoomOut),
        // Ctrl+0 — reset zoom
        KeyCode::Digit0 if ctrl_only => Some(KeyCommand::ZoomReset),
        _ => None,
    }
}
