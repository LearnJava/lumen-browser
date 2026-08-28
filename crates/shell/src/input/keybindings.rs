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

/// Where a mutable form control keeps the text it renders вЂ” the two storage
/// models HTML gives text-editing controls (BUG-436).
///
/// Picked by [`Lumen::typeable_field`] and consumed by
/// [`Lumen::edit_focused_field`], which has to write the new value back to the
/// right place: `<input>` reflects it in the `value` content attribute,
/// `<textarea>` in its text-node children (HTML LS В§4.10.11).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum TypeableField {
    /// `<input>` of a text-like type вЂ” value lives in the `value` attribute.
    Input,
    /// `<textarea>` вЂ” value lives in the element's text-node children.
    Textarea,
}

/// Р”РµР№СЃС‚РІРёСЏ shell-Р°, РЅР° РєРѕС‚РѕСЂС‹Рµ РјР°РїСЏС‚СЃСЏ РєР»Р°РІРёС€Рё. РР·РѕР»РёСЂРѕРІР°РЅС‹ РѕС‚ winit, С‡С‚РѕР±С‹
/// РјР°РїРїРµСЂ Р±С‹Р» С‚РµСЃС‚РёСЂСѓРµРј Р±РµР· event loop.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum KeyCommand {
    Reload,
    Exit,
    FindOpen,
    /// РћС‚РєСЂС‹С‚СЊ Р°РґСЂРµСЃРЅСѓСЋ СЃС‚СЂРѕРєСѓ (Ctrl+L / F6). РџРѕР·РІРѕР»СЏРµС‚ РІРІРµСЃС‚Рё URL РёР»Рё
    /// РїРѕРёСЃРєРѕРІС‹Р№ Р·Р°РїСЂРѕСЃ РїСЂСЏРјРѕ РІ Р±СЂР°СѓР·РµСЂРµ Р±РµР· РїРµСЂРµР·Р°РїСѓСЃРєР° РёР· CLI.
    OpenAddressBar,
    /// РќР°РІРёРіР°С†РёСЏ РЅР°Р·Р°Рґ (Alt+Left). Р’РѕСЃСЃС‚Р°РЅР°РІР»РёРІР°РµС‚ РёР· bfcache РµСЃР»Рё РІРѕР·РјРѕР¶РЅРѕ.
    HistoryBack,
    /// РќР°РІРёРіР°С†РёСЏ РІРїРµСЂС‘Рґ (Alt+Right). Р’РѕСЃСЃС‚Р°РЅР°РІР»РёРІР°РµС‚ РёР· bfcache РµСЃР»Рё РІРѕР·РјРѕР¶РЅРѕ.
    HistoryForward,
    /// РЎРєСЂРѕР»Р» РЅР° РѕРґРЅСѓ СЃС‚СЂРѕРєСѓ РІРЅРёР· (СЃС‚СЂРµР»РєР° РІРЅРёР·).
    ScrollLineDown,
    /// РЎРєСЂРѕР»Р» РЅР° РѕРґРЅСѓ СЃС‚СЂРѕРєСѓ РІРІРµСЂС… (СЃС‚СЂРµР»РєР° РІРЅРёР·).
    ScrollLineUp,
    /// РЎРєСЂРѕР»Р» РЅР° ~90% viewport-Р° РІРЅРёР· (PageDown / Space).
    ScrollPageDown,
    /// РЎРєСЂРѕР»Р» РЅР° ~90% viewport-Р° РІРІРµСЂС… (PageUp / Shift+Space).
    ScrollPageUp,
    /// РџСЂС‹Р¶РѕРє Рє РЅР°С‡Р°Р»Сѓ РґРѕРєСѓРјРµРЅС‚Р° (Home).
    ScrollHome,
    /// РџСЂС‹Р¶РѕРє Рє РєРѕРЅС†Сѓ РґРѕРєСѓРјРµРЅС‚Р° (End).
    ScrollEnd,
    /// Р“РѕСЂРёР·РѕРЅС‚Р°Р»СЊРЅС‹Р№ СЃРєСЂРѕР»Р» РЅР° РѕРґРЅСѓ РєРѕР»РѕРЅРєСѓ РІРїСЂР°РІРѕ (СЃС‚СЂРµР»РєР° РІРїСЂР°РІРѕ).
    ScrollLineRight,
    /// Р“РѕСЂРёР·РѕРЅС‚Р°Р»СЊРЅС‹Р№ СЃРєСЂРѕР»Р» РЅР° РѕРґРЅСѓ РєРѕР»РѕРЅРєСѓ РІР»РµРІРѕ (СЃС‚СЂРµР»РєР° РІР»РµРІРѕ).
    ScrollLineLeft,
    /// РћС‚РєСЂС‹С‚СЊ hint-СЂРµР¶РёРј: РїРѕРєР°Р·Р°С‚СЊ kbd-Р±РµР№РґР¶Рё РЅР° РІСЃРµС… РєР»РёРєР°Р±РµР»СЊРЅС‹С… СЌР»РµРјРµРЅС‚Р°С… (F).
    HintModeOpen,
    /// РћС‚РєСЂС‹С‚СЊ РЅРѕРІСѓСЋ РІРєР»Р°РґРєСѓ (Ctrl+T).
    NewTab,
    /// Р—Р°РєСЂС‹С‚СЊ С‚РµРєСѓС‰СѓСЋ РІРєР»Р°РґРєСѓ РёР»Рё РІС‹Р№С‚Рё, РµСЃР»Рё РІРєР»Р°РґРєР° РїРѕСЃР»РµРґРЅСЏСЏ (Ctrl+W).
    CloseTab,
    /// РџРµСЂРµРєР»СЋС‡РёС‚СЊСЃСЏ РЅР° СЃР»РµРґСѓСЋС‰СѓСЋ РІРєР»Р°РґРєСѓ С†РёРєР»РёС‡РµСЃРєРё (Ctrl+Tab).
    NextTab,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ РїР°РЅРµР»СЊ Р·Р°РіСЂСѓР·РѕРє (Ctrl+Shift+J).
    DownloadsPanel,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ split view (Ctrl+\): РїРѕРєР°Р·С‹РІР°РµС‚ Р°РєС‚РёРІРЅСѓСЋ Рё СЃР»РµРґСѓСЋС‰СѓСЋ
    /// РІРєР»Р°РґРєСѓ СЂСЏРґРѕРј. РџСЂРё РїРѕРІС‚РѕСЂРЅРѕРј РЅР°Р¶Р°С‚РёРё Р·Р°РєСЂС‹РІР°РµС‚ split.
    SplitView,
    /// РџРµСЂРµРєР»СЋС‡РёС‚СЊ С„РѕРєСѓСЃ РјРµР¶РґСѓ Р»РµРІРѕР№ Рё РїСЂР°РІРѕР№ РїР°РЅРµР»СЏРјРё split view (Ctrl+M).
    SplitFocusSwitch,
    /// Р’РєР»СЋС‡РёС‚СЊ/РІС‹РєР»СЋС‡РёС‚СЊ Vim-СЂРµР¶РёРј РЅР°РІРёРіР°С†РёРё (Ctrl+Alt+V).
    VimModeToggle,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РІРµСЂС‚РёРєР°Р»СЊРЅСѓСЋ РїР°РЅРµР»СЊ РІРєР»Р°РґРѕРє (Ctrl+B).
    ToggleVerticalTabs,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ tree-style РїР°РЅРµР»СЊ РІРєР»Р°РґРѕРє (Ctrl+Shift+B).
    ToggleTreeTabs,
    /// РџРµСЂРµРЅРµСЃС‚Рё Р°РєС‚РёРІРЅС‹Р№ СЃР°Р№РґР±Р°СЂ РІРєР»Р°РґРѕРє Рє РїСЂРѕС‚РёРІРѕРїРѕР»РѕР¶РЅРѕРјСѓ РєСЂР°СЋ РѕРєРЅР° (Ctrl+Alt+B).
    FlipActiveDock,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РїР°РЅРµР»СЊ РІРѕСЂРєСЃРїРµР№СЃРѕРІ (Ctrl+Shift+W).
    ToggleWorkspaces,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РїР°РЅРµР»СЊ Shields (Ctrl+Shift+S).
    ToggleShields,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РїР°РЅРµР»СЊ СЂР°Р·СЂРµС€РµРЅРёР№ СЃР°Р№С‚Р° (Ctrl+Shift+P, 7C.2).
    TogglePermissions,
    /// Р’РєР»СЋС‡РёС‚СЊ/РІС‹РєР»СЋС‡РёС‚СЊ Р°РІС‚Рѕ-Р·Р°РєСЂС‹С‚РёРµ cookie-Р±Р°РЅРЅРµСЂРѕРІ (Ctrl+Shift+K, 7C.3).
    ToggleCookieBannerDismiss,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ AI-РїР°РЅРµР»СЊ (Ctrl+Shift+A, В§12.8).
    ToggleAiPanel,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ РїР°РЅРµР»СЊ РЅР°СЃС‚СЂРѕРµРє РґРѕСЃС‚СѓРїРЅРѕСЃС‚Рё (Ctrl+Shift+Q, E-2).
    ToggleA11y,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РјРµРЅРµРґР¶РµСЂ Р·Р°РєР»Р°РґРѕРє (Ctrl+Shift+O, task #22).
    ToggleBookmarks,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РїР°РЅРµР»СЊ РёСЃС‚РѕСЂРёРё Р±СЂР°СѓР·РµСЂР° (Ctrl+H, task D-5).
    ToggleHistory,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ СЃС‚СЂР°РЅРёС†Сѓ РЅР°СЃС‚СЂРѕРµРє Р±СЂР°СѓР·РµСЂР° (Ctrl+,, task D-7).
    ToggleSettings,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РєРѕРјР°РЅРґРЅСѓСЋ РїР°Р»РёС‚СЂСѓ (Ctrl+K, В§7E.2, task #23).
    ToggleCommandPalette,
    /// Р’РѕР№С‚Рё/РІС‹Р№С‚Рё РёР· focus mode + Pomodoro (Ctrl+Shift+F, task #25, V4).
    ToggleFocusMode,
    /// Р”РѕР±Р°РІРёС‚СЊ С‚РµРєСѓС‰СѓСЋ СЃС‚СЂР°РЅРёС†Сѓ РІ Р·Р°РєР»Р°РґРєРё (Ctrl+D).
    BookmarkCurrentPage,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ DevTools JS-РєРѕРЅСЃРѕР»СЊ (F12, В§7E.5).
    DevConsole,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ DevTools DOM-РёРЅСЃРїРµРєС‚РѕСЂ (Ctrl+Shift+I, В§7E.1).
    DevInspector,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ DevTools РїР°РЅРµР»СЊ СЃРµС‚Рё (Ctrl+Shift+E, В§7E.4).
    DevNetwork,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ privacy-РїР°РЅРµР»СЊ СЃРµС‚Рё (Ctrl+Shift+Y, V5).
    TogglePrivacy,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ picture-in-picture РѕРєРЅРѕ РІРёРґРµРѕ (Ctrl+Shift+V, task #21).
    TogglePip,
    /// РџРѕРєР°Р·Р°С‚СЊ/СЃРєСЂС‹С‚СЊ РїР°РЅРµР»СЊ Read-later (Ctrl+Shift+R, В§12.3).
    ToggleReadLater,
    /// Р’РєР»СЋС‡РёС‚СЊ/РІС‹РєР»СЋС‡РёС‚СЊ Reader View (F9, В§D-3): clean article layout.
    ToggleReaderView,
    /// РћС‚РєСЂС‹С‚СЊ РїСЂРѕСЃРјРѕС‚СЂ РёСЃС…РѕРґРЅРѕРіРѕ РєРѕРґР° С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹ (Ctrl+U, В§D-2).
    ViewSource,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ РїР°РЅРµР»СЊ РіРѕСЂСЏС‡РёС… РєР»Р°РІРёС€ (Ctrl+Shift+/, В§D-4).
    ToggleShortcuts,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ РґРёР°Р»РѕРі РїРµС‡Р°С‚Рё СЃС‚СЂР°РЅРёС†С‹ (Ctrl+P, E-1).
    TogglePrint,
    /// РћС‚РєСЂС‹С‚СЊ/Р·Р°РєСЂС‹С‚СЊ РїСЂРѕСЃРјРѕС‚СЂ TLS-СЃРµСЂС‚РёС„РёРєР°С‚Р° (Ctrl+Shift+C, В§D-1).
    ToggleCert,
    /// РќР°Р·РЅР°С‡РёС‚СЊ РєРѕРЅС‚РµР№РЅРµСЂ Р°РєС‚РёРІРЅРѕР№ РІРєР»Р°РґРєРµ (7D.2). РќРµ РїСЂРёРІСЏР·Р°РЅРѕ Рє РєР»Р°РІРёС€Рµ вЂ”
    /// РґРёСЃРїР°С‚С‡РёС‚СЃСЏ РїСЂРѕРіСЂР°РјРјРЅРѕ (РєРѕРЅС‚РµРєСЃС‚РЅРѕРµ РјРµРЅСЋ РІРєР»Р°РґРєРё / omnibox-РєРѕРјР°РЅРґР°
    /// `container <name>`). РЎРј. `tabs::containers::ContainerKind`.
    ///
    /// РљРѕРЅСЃС‚СЂСѓРёСЂСѓРµС‚СЃСЏ С‡РµСЂРµР· С€РµР»Р»-РєРѕРјР°РЅРґС‹/omnibox РІ follow-up С‚Р°СЃРєРµ; РїРѕРєР°
    /// РіР°СЃРёРј dead_code-РїСЂРµРґСѓРїСЂРµР¶РґРµРЅРёРµ, С‡С‚РѕР±С‹ `cargo clippy -D warnings` РїСЂРѕС€С‘Р».
    #[allow(dead_code)]
    SetTabContainer(tabs::containers::ContainerKind),
    /// РЈРІРµР»РёС‡РёС‚СЊ РјР°СЃС€С‚Р°Р± СЃС‚СЂР°РЅРёС†С‹ (Ctrl+=).
    ZoomIn,
    /// РЈРјРµРЅСЊС€РёС‚СЊ РјР°СЃС€С‚Р°Р± СЃС‚СЂР°РЅРёС†С‹ (Ctrl+-).
    ZoomOut,
    /// РЎР±СЂРѕСЃРёС‚СЊ РјР°СЃС€С‚Р°Р± СЃС‚СЂР°РЅРёС†С‹ Рє 100% (Ctrl+0).
    ZoomReset,
}

/// РњР°РїРїРёРЅРі С„РёР·РёС‡РµСЃРєРѕР№ РєР»Р°РІРёС€Рё + РјРѕРґРёС„РёРєР°С‚РѕСЂРѕРІ РЅР° shell-action.
///
/// F5 Р±РµР· РјРѕРґРёС„РёРєР°С‚РѕСЂРѕРІ  в†’ Reload.
/// Ctrl+R                в†’ Reload.
/// Esc Р±РµР· РјРѕРґРёС„РёРєР°С‚РѕСЂРѕРІ в†’ Exit.
/// Ctrl+W                в†’ Exit.
/// Ctrl+F                в†’ FindOpen.
/// F (Р±РµР· РјРѕРґРёС„РёРєР°С‚РѕСЂРѕРІ) в†’ HintModeOpen (kbd-РЅР°РІРёРіР°С†РёСЏ РїРѕ СЃСЃС‹Р»РєР°Рј/РєРЅРѕРїРєР°Рј).
/// в†“ / в†‘                 в†’ ScrollLineDown / ScrollLineUp (Р±РµР· РјРѕРґРёС„РёРєР°С‚РѕСЂРѕРІ).
/// в†’ / в†ђ                 в†’ ScrollLineRight / ScrollLineLeft (Р±РµР· РјРѕРґРёС„РёРєР°С‚РѕСЂРѕРІ).
/// PageDown / PageUp     в†’ ScrollPageDown / ScrollPageUp.
/// Space / Shift+Space   в†’ ScrollPageDown / ScrollPageUp (РїСЂРёРІС‹С‡РєР° РїСЂРѕР±РµР»Р° РІ Р±СЂР°СѓР·РµСЂР°С…).
/// Home / End            в†’ ScrollHome / ScrollEnd.
///
/// РџСЂРѕС‡РёРµ РєРѕРјР±РёРЅР°С†РёРё (Ctrl+Shift+R, F5+Ctrl, Рё С‚.Рґ.) вЂ” РїРѕРєР° None: РЅРµ С…РѕС‚РёРј
/// РїРµСЂРµС…РІР°С‚С‹РІР°С‚СЊ РїСЂРёРІС‹С‡РЅС‹Рµ web-shortcuts (force-reload, etc.) РґРѕ С‚РѕРіРѕ, РєР°Рє
/// СЂРµС€РёРј, С‡С‚Рѕ РѕРЅРё РґРѕР»Р¶РЅС‹ РґРµР»Р°С‚СЊ.
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
        // Ctrl+\ вЂ” toggle split view (show active + next tab side-by-side)
        KeyCode::Backslash if ctrl_only => Some(KeyCommand::SplitView),
        // Ctrl+M вЂ” move focus between left / right pane in split mode
        KeyCode::KeyM if ctrl_only => Some(KeyCommand::SplitFocusSwitch),
        // Ctrl+Alt+V вЂ” toggle Vim navigation mode
        KeyCode::KeyV if mods == (ModifiersState::CONTROL | ModifiersState::ALT) => {
            Some(KeyCommand::VimModeToggle)
        }
        // Ctrl+B вЂ” toggle vertical tab sidebar
        KeyCode::KeyB if ctrl_only => Some(KeyCommand::ToggleVerticalTabs),
        // Ctrl+Shift+B вЂ” toggle tree-style tab sidebar
        KeyCode::KeyB if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleTreeTabs)
        }
        // Ctrl+Alt+B вЂ” move the active tab sidebar to the opposite edge (cross-dock)
        KeyCode::KeyB if mods == (ModifiersState::CONTROL | ModifiersState::ALT) => {
            Some(KeyCommand::FlipActiveDock)
        }
        // Ctrl+Shift+W вЂ” toggle workspace switcher bar
        KeyCode::KeyW if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleWorkspaces)
        }
        // Ctrl+Shift+S вЂ” toggle shields panel
        KeyCode::KeyS if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleShields)
        }
        // Ctrl+Shift+P вЂ” toggle per-site permission popover (7C.2)
        KeyCode::KeyP if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePermissions)
        }
        // Ctrl+Shift+K вЂ” toggle cookie-banner auto-dismiss (7C.3)
        KeyCode::KeyK if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleCookieBannerDismiss)
        }
        // Ctrl+Shift+A вЂ” toggle AI sidebar panel (В§12.8)
        KeyCode::KeyA if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleAiPanel)
        }
        // Ctrl+Shift+O вЂ” toggle bookmark manager panel (task #22)
        KeyCode::KeyO if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleBookmarks)
        }
        // Ctrl+H вЂ” toggle browser history panel (task D-5)
        KeyCode::KeyH if ctrl_only => Some(KeyCommand::ToggleHistory),
        // Ctrl+, вЂ” open browser settings (task D-7)
        KeyCode::Comma if ctrl_only => Some(KeyCommand::ToggleSettings),
        // Ctrl+Shift+F вЂ” toggle focus mode + Pomodoro timer (task #25, V4)
        KeyCode::KeyF if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleFocusMode)
        }
        // Ctrl+K вЂ” toggle the command palette (В§7E.2)
        KeyCode::KeyK if ctrl_only => Some(KeyCommand::ToggleCommandPalette),
        // Ctrl+D вЂ” bookmark the current page
        KeyCode::KeyD if ctrl_only => Some(KeyCommand::BookmarkCurrentPage),
        // F12 вЂ” toggle DevTools JS console (В§7E.5)
        KeyCode::F12 if no_mods => Some(KeyCommand::DevConsole),
        // Ctrl+Shift+I вЂ” toggle DevTools DOM inspector (В§7E.1)
        KeyCode::KeyI if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::DevInspector)
        }
        // Ctrl+Shift+E вЂ” toggle DevTools network panel (В§7E.4)
        KeyCode::KeyE if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::DevNetwork)
        }
        // Ctrl+Shift+Y вЂ” toggle privacy network panel (V5)
        KeyCode::KeyY if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePrivacy)
        }
        // Ctrl+Shift+V вЂ” toggle picture-in-picture video window (task #21)
        KeyCode::KeyV if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePip)
        }
        // Ctrl+Shift+Q вЂ” toggle accessibility settings panel (E-2)
        KeyCode::KeyQ if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleA11y)
        }
        // Ctrl+Shift+R вЂ” toggle Read-later panel (В§12.3)
        KeyCode::KeyR if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleReadLater)
        }
        // F9 вЂ” toggle Reader View (В§D-3)
        KeyCode::F9 if no_mods => Some(KeyCommand::ToggleReaderView),
        // Ctrl+U вЂ” view page source (В§D-2)
        KeyCode::KeyU if ctrl_only => Some(KeyCommand::ViewSource),
        // Ctrl+Shift+/ вЂ” toggle keyboard shortcuts panel (В§D-4)
        KeyCode::Slash if ctrl_and_shift => Some(KeyCommand::ToggleShortcuts),
        // Ctrl+P вЂ” print dialog (E-1)
        KeyCode::KeyP if ctrl_only => Some(KeyCommand::TogglePrint),
        // Ctrl+Shift+C вЂ” certificate viewer (В§D-1)
        KeyCode::KeyC if ctrl_and_shift => Some(KeyCommand::ToggleCert),
        // Ctrl+= вЂ” zoom in
        KeyCode::Equal if ctrl_only => Some(KeyCommand::ZoomIn),
        // Ctrl+- вЂ” zoom out
        KeyCode::Minus if ctrl_only => Some(KeyCommand::ZoomOut),
        // Ctrl+0 вЂ” reset zoom
        KeyCode::Digit0 if ctrl_only => Some(KeyCommand::ZoomReset),
        _ => None,
    }
}
