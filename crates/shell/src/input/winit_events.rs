//! Winit-facing input helpers: modifier state and cursor icons.
//!
//! Free functions the event loop calls while translating an OS event into
//! shell state — the `Modifiers` unwrap for shortcut matching and the two
//! `CursorIcon` mappings (scrollbar hover result, CSS `cursor` keyword).
//! Moved out of `main.rs` by the SPLIT track (batch SH-5); behaviour and
//! signatures are unchanged.

use lumen_layout::Cursor as CssCursor;
use winit::event::Modifiers;
use winit::keyboard::ModifiersState;
use winit::window::CursorIcon;

use crate::scrollbar;

/// Р”РѕСЃС‚Р°С‚СЊ С‡РёСЃС‚С‹Р№ `ModifiersState` РёР· РѕР±С‘СЂС‚РєРё `Modifiers` (winit 0.30 СЂР°Р·Р»РёС‡Р°РµС‚
/// "physical state" вЂ” Ctrl РєР°Рє РєР»Р°РІРёС€Р° вЂ” Рё "lock state"; РґР»СЏ shortcuts РЅР°Рј
/// РЅСѓР¶РЅРѕ С„РёР·РёС‡РµСЃРєРѕРµ СЃРѕСЃС‚РѕСЏРЅРёРµ).
pub(crate) fn winit_modifiers_state(mods: &Modifiers) -> ModifiersState {
    mods.state()
}

/// Pure-fn: РєР°РєРѕР№ `CursorIcon` РїРѕРєР°Р·Р°С‚СЊ РїРѕ СЂРµР·СѓР»СЊС‚Р°С‚Сѓ hit-С‚РµСЃС‚Р° scrollbar-Р°
/// Рё С„Р»Р°РіСѓ Р°РєС‚РёРІРЅРѕРіРѕ drag-Р°. `Pointer` СЃРёРіРЅР°Р»РёС‚ В«Р·РґРµСЃСЊ РёРЅС‚РµСЂР°РєС‚РёРІВ»:
/// - drag Р°РєС‚РёРІРµРЅ в†’ `Pointer` РЅРµР·Р°РІРёСЃРёРјРѕ РѕС‚ С‚РµРєСѓС‰РµР№ С‚РѕС‡РєРё (РІРёРЅРёС‚ С€Р»С‘С‚
///   CursorMoved Р·Р° РїСЂРµРґРµР»Р°РјРё РѕРєРЅР° С‚РѕР¶Рµ, Рё cursor РґРѕР»Р¶РµРЅ В«РїСЂРёР»РёРїРЅСѓС‚СЊВ»);
/// - hover thumb в†’ `Pointer`;
/// - hover track РІС‹С€Рµ/РЅРёР¶Рµ thumb-Р° РёР»Рё РєР»РёРє РјРёРјРѕ в†’ `Default` (track-click
///   С‚РѕР¶Рµ clickable, РЅРѕ cursor-change РЅР° РїСѓСЃС‚РѕРј track-Рµ Р±С‹Р» Р±С‹ С€СѓРјРЅС‹Рј вЂ”
///   СЃС‚Р°РЅРґР°СЂС‚ РІСЃРµС… Р±СЂР°СѓР·РµСЂРѕРІ).
pub(crate) fn cursor_icon_for_hover(hover: scrollbar::TrackClick, drag_active: bool) -> CursorIcon {
    if drag_active {
        return CursorIcon::Pointer;
    }
    match hover {
        scrollbar::TrackClick::Thumb => CursorIcon::Pointer,
        _ => CursorIcon::Default,
    }
}

/// РљРѕРЅРІРµСЂС‚РёСЂСѓРµС‚ CSS `cursor` keyword РІ winit `CursorIcon`.
/// `Auto` в†’ `Default` (UA-СЂРµС€РµРЅРёРµ РґР»СЏ Phase 0); `None` в†’ `Default` (winit РЅРµ
/// РїРѕРґРґРµСЂР¶РёРІР°РµС‚ В«СЃРєСЂС‹С‚С‹Р№ РєСѓСЂСЃРѕСЂВ» С‡РµСЂРµР· CursorIcon вЂ” РЅСѓР¶РµРЅ РѕС‚РґРµР»СЊРЅС‹Р№ API).
pub(crate) fn css_cursor_to_winit(c: CssCursor) -> CursorIcon {
    match c {
        CssCursor::Auto | CssCursor::Default => CursorIcon::Default,
        CssCursor::None => CursorIcon::Default,
        CssCursor::ContextMenu => CursorIcon::ContextMenu,
        CssCursor::Help => CursorIcon::Help,
        CssCursor::Pointer => CursorIcon::Pointer,
        CssCursor::Progress => CursorIcon::Progress,
        CssCursor::Wait => CursorIcon::Wait,
        CssCursor::Cell => CursorIcon::Cell,
        CssCursor::Crosshair => CursorIcon::Crosshair,
        CssCursor::Text => CursorIcon::Text,
        CssCursor::VerticalText => CursorIcon::VerticalText,
        CssCursor::Alias => CursorIcon::Alias,
        CssCursor::Copy => CursorIcon::Copy,
        CssCursor::Move => CursorIcon::Move,
        CssCursor::NoDrop => CursorIcon::NoDrop,
        CssCursor::NotAllowed => CursorIcon::NotAllowed,
        CssCursor::Grab => CursorIcon::Grab,
        CssCursor::Grabbing => CursorIcon::Grabbing,
        CssCursor::AllScroll => CursorIcon::AllScroll,
        CssCursor::ColResize => CursorIcon::ColResize,
        CssCursor::RowResize => CursorIcon::RowResize,
        CssCursor::NResize => CursorIcon::NResize,
        CssCursor::EResize => CursorIcon::EResize,
        CssCursor::SResize => CursorIcon::SResize,
        CssCursor::WResize => CursorIcon::WResize,
        CssCursor::NeResize => CursorIcon::NeResize,
        CssCursor::NwResize => CursorIcon::NwResize,
        CssCursor::SeResize => CursorIcon::SeResize,
        CssCursor::SwResize => CursorIcon::SwResize,
        CssCursor::EwResize => CursorIcon::EwResize,
        CssCursor::NsResize => CursorIcon::NsResize,
        CssCursor::NeswResize => CursorIcon::NeswResize,
        CssCursor::NwseResize => CursorIcon::NwseResize,
        CssCursor::ZoomIn => CursorIcon::ZoomIn,
        CssCursor::ZoomOut => CursorIcon::ZoomOut,
    }
}
