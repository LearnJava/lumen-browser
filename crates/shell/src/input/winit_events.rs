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

/// Достать чистый `ModifiersState` из обёртки `Modifiers` (winit 0.30 различает
/// "physical state" — Ctrl как клавиша — и "lock state"; для shortcuts нам
/// нужно физическое состояние).
pub(crate) fn winit_modifiers_state(mods: &Modifiers) -> ModifiersState {
    mods.state()
}

/// Pure-fn: какой `CursorIcon` показать по результату hit-теста scrollbar-а
/// и флагу активного drag-а. `Pointer` сигналит «здесь интерактив»:
/// - drag активен → `Pointer` независимо от текущей точки (винит шлёт
///   CursorMoved за пределами окна тоже, и cursor должен «прилипнуть»);
/// - hover thumb → `Pointer`;
/// - hover track выше/ниже thumb-а или клик мимо → `Default` (track-click
///   тоже clickable, но cursor-change на пустом track-е был бы шумным —
///   стандарт всех браузеров).
pub(crate) fn cursor_icon_for_hover(hover: scrollbar::TrackClick, drag_active: bool) -> CursorIcon {
    if drag_active {
        return CursorIcon::Pointer;
    }
    match hover {
        scrollbar::TrackClick::Thumb => CursorIcon::Pointer,
        _ => CursorIcon::Default,
    }
}

/// Конвертирует CSS `cursor` keyword в winit `CursorIcon`.
/// `Auto` → `Default` (UA-решение для Phase 0); `None` → `Default` (winit не
/// поддерживает «скрытый курсор» через CursorIcon — нужен отдельный API).
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
