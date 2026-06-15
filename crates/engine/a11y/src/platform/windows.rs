//! Windows UI Automation bridge — Phase 1.
//!
//! Phase 0 (complete): holds the last-known `AXTree` in memory; no OS calls.
//!
//! Phase 1 (this file): real Win32 accessibility events via MSAA:
//!   - `init_hwnd(hwnd)` — stores the shell's `HWND` for subsequent OS calls.
//!   - `update(tree)` — fires `EVENT_OBJECT_REORDER` when the tree changes,
//!     telling screen readers (NVDA, JAWS, Narrator) to re-query the page.
//!   - `focused_node_changed(node_id)` — fires `EVENT_OBJECT_FOCUS` and
//!     `EVENT_OBJECT_STATECHANGE` so the AT announces the newly focused element.
//!   - `handle_wm_get_object(wparam, lparam)` — when the shell receives
//!     `WM_GETOBJECT` with `OBJID_CLIENT`, returns an `LresultFromObject` LRESULT
//!     for the accessible root via `CreateStdAccessibleObject`.
//!   - `shutdown()` — revokes the HWND so no further OS calls are made.
//!
//! Phase 2 (not yet): full `IRawElementProviderSimple` COM provider exposing every
//!   `AXNode` as a named UIA element (requires `windows` crate `implement!` macro).
//!
//! Win32 event IDs used:
//!   EVENT_OBJECT_FOCUS        = 0x8005  (element gained focus)
//!   EVENT_OBJECT_REORDER      = 0x8004  (tree structure changed)
//!   EVENT_OBJECT_STATECHANGE  = 0x800A  (element state changed)
//!   OBJID_CLIENT              = -4      (identifies the window client area)
//!   CHILDID_SELF              = 0       (the object itself, not a child)

use crate::{AXRole, AXTree, NodeId};
use super::PlatformBridge;

// ── Win32 constants ───────────────────────────────────────────────────────────

/// `EVENT_OBJECT_FOCUS` — element received focus.
const EVENT_OBJECT_FOCUS: u32 = 0x8005;
/// `EVENT_OBJECT_REORDER` — children of object changed order/count.
const EVENT_OBJECT_REORDER: u32 = 0x8004;
/// `EVENT_OBJECT_STATECHANGE` — element state changed.
const EVENT_OBJECT_STATECHANGE: u32 = 0x800A;
/// `OBJID_CLIENT` — the object ID for the window's client area.
const OBJID_CLIENT: i32 = -4_i32;
/// `CHILDID_SELF` — the event targets the object itself, not a child.
const CHILDID_SELF: i32 = 0;

// ── WinUiaBridge ─────────────────────────────────────────────────────────────

/// Windows UI Automation bridge.
///
/// Phase 1: fires Win32 `NotifyWinEvent` calls when the AX tree or focus changes,
/// and handles `WM_GETOBJECT` by returning an MSAA accessible root.
/// Phase 0 behaviour is preserved when no HWND has been registered.
pub struct WinUiaBridge {
    /// Shell window handle (`HWND`). `0` means not yet initialised or already shut down.
    hwnd: isize,
    /// Most recent accessibility tree. `None` before the first page load.
    tree: Option<AXTree>,
    /// Currently focused DOM node. `None` if focus is outside web content.
    focused: Option<NodeId>,
    /// Sequence number incremented on every `update()`. Used to suppress redundant events.
    update_seq: u32,
}

impl WinUiaBridge {
    /// Create a new, uninitialised UIA bridge.
    ///
    /// Call [`PlatformBridge::init_hwnd`] with the shell `HWND` before any page
    /// loads; otherwise the bridge operates in Phase-0 no-op mode.
    pub fn new() -> Self {
        Self {
            hwnd: 0,
            tree: None,
            focused: None,
            update_seq: 0,
        }
    }

    /// Return the last-received accessibility tree, if any.
    pub fn last_tree(&self) -> Option<&AXTree> {
        self.tree.as_ref()
    }

    /// Return the currently focused node, if any.
    pub fn focused_node(&self) -> Option<NodeId> {
        self.focused
    }

    /// Emit a Win32 accessibility event via `NotifyWinEvent`.
    ///
    /// No-op when `hwnd == 0` (Phase 0 / headless mode).
    #[cfg(target_os = "windows")]
    fn notify(&self, event: u32, id_object: i32, id_child: i32) {
        if self.hwnd == 0 {
            return;
        }
        // SAFETY: hwnd was obtained from winit's raw_window_handle and stored in
        // init_hwnd(). NotifyWinEvent is safe to call from any thread.
        unsafe {
            windows_sys::Win32::UI::Accessibility::NotifyWinEvent(
                event,
                self.hwnd as windows_sys::Win32::Foundation::HWND,
                id_object,
                id_child,
            );
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn notify(&self, _event: u32, _id_object: i32, _id_child: i32) {}

    /// Map a DOM node's 0-based index to a 1-based MSAA child ID.
    ///
    /// MSAA child IDs are 1-based; `CHILDID_SELF` (0) is reserved for the root.
    fn child_id_for(node_id: NodeId) -> i32 {
        (node_id.index() as i32).saturating_add(1)
    }
}

impl Default for WinUiaBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBridge for WinUiaBridge {
    fn update(&mut self, tree: &AXTree) {
        let was_some = self.tree.is_some();
        self.tree = Some(tree.clone());
        self.update_seq = self.update_seq.wrapping_add(1);

        if was_some {
            // Structure changed — tell screen readers to re-query children.
            self.notify(EVENT_OBJECT_REORDER, OBJID_CLIENT, CHILDID_SELF);
        }
    }

    fn focused_node_changed(&mut self, node_id: Option<NodeId>) {
        self.focused = node_id;

        match node_id {
            Some(nid) => {
                let child = Self::child_id_for(nid);
                self.notify(EVENT_OBJECT_FOCUS, OBJID_CLIENT, child);
                self.notify(EVENT_OBJECT_STATECHANGE, OBJID_CLIENT, child);
            }
            None => {
                // Focus left web content.
                self.notify(EVENT_OBJECT_FOCUS, OBJID_CLIENT, CHILDID_SELF);
            }
        }
    }

    fn shutdown(&mut self) {
        self.hwnd = 0;
        self.tree = None;
        self.focused = None;
    }

    fn init_hwnd(&mut self, hwnd: isize) {
        self.hwnd = hwnd;
    }

    /// Handle `WM_GETOBJECT` for the client-area object ID.
    ///
    /// When `lparam == OBJID_CLIENT`, returns an LRESULT wrapping a basic MSAA
    /// accessible root obtained via `CreateStdAccessibleObject` + `LresultFromObject`.
    /// Returns `None` for all other object IDs.
    #[cfg(target_os = "windows")]
    fn handle_wm_get_object(&mut self, wparam: usize, lparam: isize) -> Option<isize> {
        if lparam != OBJID_CLIENT as isize || self.hwnd == 0 {
            return None;
        }

        // SAFETY: hwnd is valid (set by init_hwnd from winit's raw_window_handle).
        // CreateStdAccessibleObject is safe for any valid HWND + OBJID_CLIENT.
        // LresultFromObject adds a reference, so we Release our local ref.
        unsafe {
            use windows_sys::Win32::UI::Accessibility::{
                CreateStdAccessibleObject, LresultFromObject,
            };
            use windows_sys::Win32::Foundation::HWND;
            use windows_sys::core::GUID;

            // IID_IAccessible = {618736E0-3C3D-11CF-810C-00AA00389B71}
            let iid_iaccessible = GUID {
                data1: 0x618736E0,
                data2: 0x3C3D,
                data3: 0x11CF,
                data4: [0x81, 0x0C, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
            };

            let mut punk: *mut std::ffi::c_void = std::ptr::null_mut();
            let hr = CreateStdAccessibleObject(
                self.hwnd as HWND,
                OBJID_CLIENT,
                &iid_iaccessible,
                &mut punk,
            );

            if hr != 0 || punk.is_null() {
                return None;
            }

            let lr = LresultFromObject(&iid_iaccessible, wparam, punk as *mut _);

            // Release our reference; LresultFromObject holds its own.
            let vtable = *(punk as *mut *mut IUnknownVtbl);
            ((*vtable).release)(punk);

            Some(lr)
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn handle_wm_get_object(&mut self, _wparam: usize, _lparam: isize) -> Option<isize> {
        None
    }
}

/// Minimal IUnknown vtable layout used to call `release` on a raw COM pointer.
///
/// Field order matches the COM ABI: QueryInterface, AddRef, Release.
#[cfg(target_os = "windows")]
#[repr(C)]
struct IUnknownVtbl {
    query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const windows_sys::core::GUID,
        *mut *mut std::ffi::c_void,
    ) -> windows_sys::core::HRESULT,
    add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
}

// ── AXRole → MSAA role mapping ───────────────────────────────────────────────

/// Map a Lumen `AXRole` to a Windows MSAA `ROLE_SYSTEM_*` constant.
///
/// Used for Phase 2 `IAccessible::get_accRole` implementation.
/// Constants from `oleacc.h` (ROLE_SYSTEM_* values).
#[allow(dead_code)]
pub fn ax_role_to_msaa(role: AXRole) -> u32 {
    match role {
        // ── Document / landmark ──────────────────────────────────────────────
        AXRole::Document    => 0x000F, // ROLE_SYSTEM_DOCUMENT
        AXRole::Article     => 0x000F, // ROLE_SYSTEM_DOCUMENT
        AXRole::Banner      => 0x001C, // ROLE_SYSTEM_GROUPING
        AXRole::Complementary => 0x001C,
        AXRole::ContentInfo => 0x001C,
        AXRole::Form        => 0x001C,
        AXRole::Main        => 0x001C,
        AXRole::Navigation  => 0x001C,
        AXRole::Region      => 0x001C,
        AXRole::Search      => 0x001C,

        // ── Document structure ───────────────────────────────────────────────
        AXRole::Heading     => 0x0019, // ROLE_SYSTEM_COLUMNHEADER (closest match)
        AXRole::List        => 0x0021, // ROLE_SYSTEM_LIST
        AXRole::ListItem    => 0x0022, // ROLE_SYSTEM_LISTITEM
        AXRole::Figure      => 0x0028, // ROLE_SYSTEM_GRAPHIC
        AXRole::Img         => 0x0028, // ROLE_SYSTEM_GRAPHIC
        AXRole::Presentation => 0x001C,
        AXRole::Table       => 0x001B, // ROLE_SYSTEM_TABLE
        AXRole::Row         => 0x001A, // ROLE_SYSTEM_ROW
        AXRole::Cell        => 0x001D, // ROLE_SYSTEM_CELL
        AXRole::ColumnHeader => 0x0019, // ROLE_SYSTEM_COLUMNHEADER
        AXRole::RowGroup    => 0x001C,
        AXRole::Caption     => 0x0029, // ROLE_SYSTEM_STATICTEXT
        AXRole::Group       => 0x001C,
        AXRole::Button      => 0x002B, // ROLE_SYSTEM_PUSHBUTTON
        AXRole::Term        => 0x0029, // ROLE_SYSTEM_STATICTEXT
        AXRole::Definition  => 0x0029,
        AXRole::DescriptionListDetail => 0x0029,
        AXRole::Blockquote  => 0x001C,
        AXRole::Code        => 0x0029,
        AXRole::Deletion    => 0x0029,
        AXRole::Insertion   => 0x0029,
        AXRole::Emphasis    => 0x0029,
        AXRole::Strong      => 0x0029,
        AXRole::Mark        => 0x0029,
        AXRole::Subscript   => 0x0029,
        AXRole::Superscript => 0x0029,
        AXRole::Separator   => 0x0015, // ROLE_SYSTEM_SEPARATOR
        AXRole::Time        => 0x0029,

        // ── Widgets ──────────────────────────────────────────────────────────
        AXRole::Link        => 0x001E, // ROLE_SYSTEM_LINK
        AXRole::Checkbox    => 0x002C, // ROLE_SYSTEM_CHECKBUTTON
        AXRole::Radio       => 0x002D, // ROLE_SYSTEM_RADIOBUTTON
        AXRole::TextBox     => 0x002A, // ROLE_SYSTEM_TEXT
        AXRole::ComboBox    => 0x002E, // ROLE_SYSTEM_COMBOBOX
        AXRole::ListBox     => 0x0021,
        AXRole::Option      => 0x0022,
        AXRole::Status      => 0x001B,
        AXRole::Progressbar => 0x0030, // ROLE_SYSTEM_PROGRESSBAR
        AXRole::Meter       => 0x0030,
        AXRole::Slider      => 0x0033, // ROLE_SYSTEM_SLIDER
        AXRole::Spinbutton  => 0x0034, // ROLE_SYSTEM_SPINBUTTON
        AXRole::Dialog      => 0x0012, // ROLE_SYSTEM_DIALOG
        AXRole::Menu        => 0x000B, // ROLE_SYSTEM_MENUPOPUP
        AXRole::MenuItem    => 0x000C, // ROLE_SYSTEM_MENUITEM
        AXRole::Alert       => 0x0008, // ROLE_SYSTEM_ALERT
        AXRole::AlertDialog => 0x0012,
        AXRole::Application => 0x000E, // ROLE_SYSTEM_APPLICATION
        AXRole::Feed        => 0x001C,
        AXRole::Log         => 0x001C,
        AXRole::Marquee     => 0x0029,
        AXRole::Note        => 0x001C,
        AXRole::RowHeader   => 0x001A,
        AXRole::Searchbox   => 0x002A, // ROLE_SYSTEM_TEXT
        AXRole::Switch      => 0x002C,
        AXRole::Tab         => 0x0025, // ROLE_SYSTEM_PAGETAB
        AXRole::TabList     => 0x0024, // ROLE_SYSTEM_PAGETABLIST
        AXRole::TabPanel    => 0x0028,
        AXRole::Timer       => 0x0029,
        AXRole::Toolbar     => 0x0016, // ROLE_SYSTEM_TOOLBAR
        AXRole::Tooltip     => 0x000D, // ROLE_SYSTEM_TOOLTIP
        AXRole::Tree        => 0x0023, // ROLE_SYSTEM_OUTLINE
        AXRole::TreeItem    => 0x0024, // ROLE_SYSTEM_OUTLINEITEM

        // ── Fallback ─────────────────────────────────────────────────────────
        AXRole::Generic     => 0x001C,
        AXRole::None        => 0x001C,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AXNode, AXRole, AXState};

    fn make_tree(role: AXRole) -> AXTree {
        AXTree {
            root: AXNode {
                node_id: NodeId::from_index(0),
                role,
                name: "root".to_string(),
                description: String::new(),
                placeholder: String::new(),
                state: AXState::default(),
                children: Vec::new(),
                controls: None,
                owns: Vec::new(),
                flow_to: Vec::new(),
                details: None,
            },
        }
    }

    #[test]
    fn new_bridge_has_no_tree() {
        let b = WinUiaBridge::new();
        assert!(b.last_tree().is_none());
        assert!(b.focused_node().is_none());
    }

    #[test]
    fn update_stores_tree() {
        let mut b = WinUiaBridge::new();
        b.update(&make_tree(AXRole::Document));
        assert!(b.last_tree().is_some());
        assert_eq!(b.last_tree().unwrap().root.role, AXRole::Document);
    }

    #[test]
    fn focused_node_changed_tracks_focus() {
        let mut b = WinUiaBridge::new();
        let nid = NodeId::from_index(5);
        b.focused_node_changed(Some(nid));
        assert_eq!(b.focused_node(), Some(nid));
        b.focused_node_changed(None);
        assert_eq!(b.focused_node(), None);
    }

    #[test]
    fn shutdown_clears_state() {
        let mut b = WinUiaBridge::new();
        b.update(&make_tree(AXRole::Document));
        b.focused_node_changed(Some(NodeId::from_index(1)));
        b.shutdown();
        assert!(b.last_tree().is_none());
        assert!(b.focused_node().is_none());
        assert_eq!(b.hwnd, 0);
    }

    #[test]
    fn init_hwnd_stores_value() {
        let mut b = WinUiaBridge::new();
        b.init_hwnd(0x1234);
        assert_eq!(b.hwnd, 0x1234);
    }

    #[test]
    fn wm_get_object_unknown_id_returns_none() {
        let mut b = WinUiaBridge::new();
        assert_eq!(b.handle_wm_get_object(0, 99), None);
    }

    #[test]
    fn wm_get_object_no_hwnd_returns_none() {
        let mut b = WinUiaBridge::new();
        // OBJID_CLIENT but no HWND registered.
        assert_eq!(b.handle_wm_get_object(0, OBJID_CLIENT as isize), None);
    }

    #[test]
    fn ax_role_document_maps_to_role_system_document() {
        assert_eq!(ax_role_to_msaa(AXRole::Document), 0x000F);
    }

    #[test]
    fn ax_role_button_maps_to_pushbutton() {
        assert_eq!(ax_role_to_msaa(AXRole::Button), 0x002B);
    }

    #[test]
    fn ax_role_link_maps_to_link() {
        assert_eq!(ax_role_to_msaa(AXRole::Link), 0x001E);
    }

    #[test]
    fn ax_role_textbox_maps_to_text() {
        assert_eq!(ax_role_to_msaa(AXRole::TextBox), 0x002A);
    }

    #[test]
    fn child_id_for_zero_index_is_one() {
        assert_eq!(WinUiaBridge::child_id_for(NodeId::from_index(0)), 1);
    }

    #[test]
    fn child_id_for_nonzero() {
        assert_eq!(WinUiaBridge::child_id_for(NodeId::from_index(41)), 42);
    }

    #[test]
    fn update_increments_seq() {
        let mut b = WinUiaBridge::new();
        assert_eq!(b.update_seq, 0);
        b.update(&make_tree(AXRole::Document));
        assert_eq!(b.update_seq, 1);
        b.update(&make_tree(AXRole::Document));
        assert_eq!(b.update_seq, 2);
    }
}
