//! ARIA role enum and HTML → implicit-role mapping.
//!
//! Covers all roles from WAI-ARIA 1.2 §5 (landmark, widget, document structure,
//! and window roles). The `implicit_role` function implements the
//! "Implicit WAI-ARIA Semantics" table from the HTML-AAM specification.

use serde::{Deserialize, Serialize};
use lumen_dom::{InputType, Node};

/// All WAI-ARIA 1.2 roles.
///
/// Roles not listed in this enum fall back to `AXRole::Generic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AXRole {
    // ── Landmark roles ───────────────────────────────────────────────────────
    /// `<nav>` — set of navigation links.
    Navigation,
    /// `<main>` — main content of the page.
    Main,
    /// `<aside>` — complementary content.
    Complementary,
    /// `<header>` in the body context — page banner.
    Banner,
    /// `<footer>` in the body context — page content info.
    ContentInfo,
    /// `<form>` with accessible name — form landmark.
    Form,
    /// `<search>` — search landmark.
    Search,
    /// `<section>` with accessible name — region landmark.
    Region,

    // ── Document structure roles ──────────────────────────────────────────────
    /// `<article>` — self-contained composition.
    Article,
    /// `<h1>`–`<h6>` — heading (level stored in AXState.level).
    Heading,
    /// `<ul>` / `<ol>` — list of items.
    List,
    /// `<li>` — list item.
    ListItem,
    /// `<figure>` — figure with optional caption.
    Figure,
    /// `<img alt="...">` — image with alternative text.
    Img,
    /// `<img alt="">` — decorative image (no AT announcement).
    Presentation,
    /// `<table>` — table.
    Table,
    /// `<tr>` — table row.
    Row,
    /// `<th>` / `<td>` — table cell.
    Cell,
    /// `<th>` — column/row header cell.
    ColumnHeader,
    /// `<rowgroup>`, `<thead>`, `<tbody>`, `<tfoot>` — row group.
    RowGroup,
    /// `<caption>` — table caption.
    Caption,
    /// `<details>` — disclosure widget group.
    Group,
    /// `<summary>` — disclosure widget toggle button.
    Button,
    /// `<dl>` — description list.
    Term,
    /// `<dt>` — term in a description list.
    Definition,
    /// `<dd>` — description / definition in a description list.
    // Mapped to `definition` in ARIA but kept distinct here for clarity.
    DescriptionListDetail,
    /// `<blockquote>` — block quotation.
    Blockquote,
    /// `<code>` — code fragment.
    Code,
    /// `<del>` — deleted text.
    Deletion,
    /// `<ins>` — inserted text.
    Insertion,
    /// `<em>` — emphasis.
    Emphasis,
    /// `<strong>` — strong importance.
    Strong,
    /// `<mark>` — highlighted text.
    Mark,
    /// `<sub>` — subscript.
    Subscript,
    /// `<sup>` — superscript.
    Superscript,
    /// `<hr>` — thematic separator.
    Separator,
    /// `<time>` — machine-readable time value.
    Time,

    // ── Widget roles ──────────────────────────────────────────────────────────
    /// `<a href="...">` — hyperlink.
    Link,
    /// `<button>`, `<input type="button/submit/reset/image">`.
    // Reuse Button above for summary; the name collision is intentional.
    // ↑ Already defined above as `Button` — both Summary and button map here.
    /// `<input type="checkbox">`.
    Checkbox,
    /// `<input type="radio">`.
    Radio,
    /// `<input type="text/email/…">` — single-line text box.
    TextBox,
    /// `<textarea>` — multi-line text box.
    // Uses TextBox with multiline=true in state.
    /// `<select>` without `multiple` and without `size > 1`.
    ComboBox,
    /// `<select multiple>` or `<select size="N">`.
    ListBox,
    /// `<option>` inside a listbox.
    Option,
    /// `<optgroup>` — group of options.
    // Generic (no explicit ARIA equivalent).
    /// `<output>` — live region output.
    Status,
    /// `<progress>` — progress bar.
    Progressbar,
    /// `<meter>` — scalar gauge.
    Meter,
    /// `<input type="range">` — slider.
    Slider,
    /// `<input type="number">` — spinbutton.
    Spinbutton,
    /// `<dialog>` / `<input type="color">` popups.
    Dialog,
    /// `<menu>` — context/toolbar menu.
    Menu,
    /// `<menuitem>` — menu item.
    MenuItem,
    /// `<img>` with `usemap` — image map (composite widget).
    // Mapped to `img` role; children are `<area>` links.
    /// Disclosure summary (already mapped via Button above for <summary>).

    // ── Widget roles (continued) ──────────────────────────────────────────────
    /// `role="alert"` — error/warning message.
    Alert,
    /// `role="alertdialog"` — alert dialog.
    AlertDialog,
    /// `role="application"` — application widget.
    Application,
    /// `role="feed"` — dynamic content feed (e.g. social media).
    Feed,
    /// `role="log"` — live log region.
    Log,
    /// `role="marquee"` — scrolling text.
    Marquee,
    /// `role="note"` — supplementary note.
    Note,
    /// `role="rowheader"` — row header cell (like <th scope="row">).
    RowHeader,
    /// `role="searchbox"` — search field (like <input type="search">).
    Searchbox,
    /// `role="switch"` — toggle switch (like <input type="checkbox"> styled as switch).
    Switch,
    /// `role="tab"` — tab in a tablist.
    Tab,
    /// `role="tablist"` — container for tabs.
    TabList,
    /// `role="tabpanel"` — panel associated with a tab.
    TabPanel,
    /// `role="timer"` — countdown/elapsed time display.
    Timer,
    /// `role="toolbar"` — toolbar widget.
    Toolbar,
    /// `role="tooltip"` — tooltip widget.
    Tooltip,
    /// `role="tree"` — tree widget.
    Tree,
    /// `role="treeitem"` — item in a tree widget.
    TreeItem,

    // ── Generic / fallback ────────────────────────────────────────────────────
    /// Any element with no meaningful ARIA role (div, span, p, etc.).
    Generic,
    /// `<html>` document root / body document structure.
    Document,
    /// Explicit `role="none"` / `role="presentation"`.
    None,
}

impl AXRole {
    /// Parse a WAI-ARIA role string (case-insensitive).
    ///
    /// Returns `None` for unknown role strings.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            s if s.eq_ignore_ascii_case("navigation") => Self::Navigation,
            s if s.eq_ignore_ascii_case("main") => Self::Main,
            s if s.eq_ignore_ascii_case("complementary") => Self::Complementary,
            s if s.eq_ignore_ascii_case("banner") => Self::Banner,
            s if s.eq_ignore_ascii_case("contentinfo") => Self::ContentInfo,
            s if s.eq_ignore_ascii_case("form") => Self::Form,
            s if s.eq_ignore_ascii_case("search") => Self::Search,
            s if s.eq_ignore_ascii_case("region") => Self::Region,
            s if s.eq_ignore_ascii_case("article") => Self::Article,
            s if s.eq_ignore_ascii_case("heading") => Self::Heading,
            s if s.eq_ignore_ascii_case("list") => Self::List,
            s if s.eq_ignore_ascii_case("listitem") => Self::ListItem,
            s if s.eq_ignore_ascii_case("figure") => Self::Figure,
            s if s.eq_ignore_ascii_case("img") => Self::Img,
            s if s.eq_ignore_ascii_case("presentation") => Self::Presentation,
            s if s.eq_ignore_ascii_case("none") => Self::None,
            s if s.eq_ignore_ascii_case("table") => Self::Table,
            s if s.eq_ignore_ascii_case("row") => Self::Row,
            s if s.eq_ignore_ascii_case("cell") => Self::Cell,
            s if s.eq_ignore_ascii_case("columnheader") => Self::ColumnHeader,
            s if s.eq_ignore_ascii_case("rowgroup") => Self::RowGroup,
            s if s.eq_ignore_ascii_case("caption") => Self::Caption,
            s if s.eq_ignore_ascii_case("group") => Self::Group,
            s if s.eq_ignore_ascii_case("button") => Self::Button,
            s if s.eq_ignore_ascii_case("term") => Self::Term,
            s if s.eq_ignore_ascii_case("definition") => Self::Definition,
            s if s.eq_ignore_ascii_case("blockquote") => Self::Blockquote,
            s if s.eq_ignore_ascii_case("code") => Self::Code,
            s if s.eq_ignore_ascii_case("deletion") => Self::Deletion,
            s if s.eq_ignore_ascii_case("insertion") => Self::Insertion,
            s if s.eq_ignore_ascii_case("emphasis") => Self::Emphasis,
            s if s.eq_ignore_ascii_case("strong") => Self::Strong,
            s if s.eq_ignore_ascii_case("mark") => Self::Mark,
            s if s.eq_ignore_ascii_case("subscript") => Self::Subscript,
            s if s.eq_ignore_ascii_case("superscript") => Self::Superscript,
            s if s.eq_ignore_ascii_case("separator") => Self::Separator,
            s if s.eq_ignore_ascii_case("time") => Self::Time,
            s if s.eq_ignore_ascii_case("link") => Self::Link,
            s if s.eq_ignore_ascii_case("checkbox") => Self::Checkbox,
            s if s.eq_ignore_ascii_case("radio") => Self::Radio,
            s if s.eq_ignore_ascii_case("textbox") => Self::TextBox,
            s if s.eq_ignore_ascii_case("combobox") => Self::ComboBox,
            s if s.eq_ignore_ascii_case("listbox") => Self::ListBox,
            s if s.eq_ignore_ascii_case("option") => Self::Option,
            s if s.eq_ignore_ascii_case("status") => Self::Status,
            s if s.eq_ignore_ascii_case("progressbar") => Self::Progressbar,
            s if s.eq_ignore_ascii_case("meter") => Self::Meter,
            s if s.eq_ignore_ascii_case("slider") => Self::Slider,
            s if s.eq_ignore_ascii_case("spinbutton") => Self::Spinbutton,
            s if s.eq_ignore_ascii_case("dialog") => Self::Dialog,
            s if s.eq_ignore_ascii_case("menu") => Self::Menu,
            s if s.eq_ignore_ascii_case("menuitem") => Self::MenuItem,
            s if s.eq_ignore_ascii_case("alert") => Self::Alert,
            s if s.eq_ignore_ascii_case("alertdialog") => Self::AlertDialog,
            s if s.eq_ignore_ascii_case("application") => Self::Application,
            s if s.eq_ignore_ascii_case("feed") => Self::Feed,
            s if s.eq_ignore_ascii_case("log") => Self::Log,
            s if s.eq_ignore_ascii_case("marquee") => Self::Marquee,
            s if s.eq_ignore_ascii_case("note") => Self::Note,
            s if s.eq_ignore_ascii_case("rowheader") => Self::RowHeader,
            s if s.eq_ignore_ascii_case("searchbox") => Self::Searchbox,
            s if s.eq_ignore_ascii_case("switch") => Self::Switch,
            s if s.eq_ignore_ascii_case("tab") => Self::Tab,
            s if s.eq_ignore_ascii_case("tablist") => Self::TabList,
            s if s.eq_ignore_ascii_case("tabpanel") => Self::TabPanel,
            s if s.eq_ignore_ascii_case("timer") => Self::Timer,
            s if s.eq_ignore_ascii_case("toolbar") => Self::Toolbar,
            s if s.eq_ignore_ascii_case("tooltip") => Self::Tooltip,
            s if s.eq_ignore_ascii_case("tree") => Self::Tree,
            s if s.eq_ignore_ascii_case("treeitem") => Self::TreeItem,
            s if s.eq_ignore_ascii_case("generic") => Self::Generic,
            s if s.eq_ignore_ascii_case("document") => Self::Document,
            _ => return None,
        })
    }
}

/// Compute the implicit WAI-ARIA role for a DOM node per HTML-AAM §5.
///
/// Returns `AXRole::Generic` for elements with no meaningful semantic role
/// (e.g., `<div>`, `<span>`). Returns `AXRole::Generic` for non-element nodes.
pub fn implicit_role(node: &Node) -> AXRole {
    let name = match node.element_name() {
        Some(n) => n.local.as_str(),
        None => return AXRole::Generic,
    };

    match name {
        // ── Landmarks ────────────────────────────────────────────────────────
        "nav" => AXRole::Navigation,
        "main" => AXRole::Main,
        "aside" => AXRole::Complementary,
        // <header> and <footer> are banner/contentinfo only as direct body children;
        // inside sectioning elements they become generic. Simplified here to always map.
        "header" => AXRole::Banner,
        "footer" => AXRole::ContentInfo,
        "form" => AXRole::Form,
        "search" => AXRole::Search,
        // <section> is a region only when it has an accessible name; otherwise generic.
        // We conservatively map to Region; the caller can downgrade if name is empty.
        "section" => AXRole::Region,

        // ── Document structure ────────────────────────────────────────────────
        "article" => AXRole::Article,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => AXRole::Heading,
        "ul" | "ol" => AXRole::List,
        "li" => AXRole::ListItem,
        "figure" => AXRole::Figure,
        "table" => AXRole::Table,
        "tr" => AXRole::Row,
        "td" => AXRole::Cell,
        "th" => AXRole::ColumnHeader,
        "thead" | "tbody" | "tfoot" => AXRole::RowGroup,
        "caption" => AXRole::Caption,
        "details" => AXRole::Group,
        "summary" => AXRole::Button,
        "dl" => AXRole::List,
        "dt" => AXRole::Term,
        "dd" => AXRole::DescriptionListDetail,
        "blockquote" => AXRole::Blockquote,
        "code" => AXRole::Code,
        "del" => AXRole::Deletion,
        "ins" => AXRole::Insertion,
        "em" => AXRole::Emphasis,
        "strong" => AXRole::Strong,
        "mark" => AXRole::Mark,
        "sub" => AXRole::Subscript,
        "sup" => AXRole::Superscript,
        "hr" => AXRole::Separator,
        "time" => AXRole::Time,

        // ── Widgets ───────────────────────────────────────────────────────────
        "a" | "area" => {
            // Link role only when `href` is present; otherwise generic.
            if node.get_attr("href").is_some() {
                AXRole::Link
            } else {
                AXRole::Generic
            }
        }
        "button" => AXRole::Button,
        "img" => {
            // alt="" → decorative (Presentation); alt missing or non-empty → Img.
            match node.get_attr("alt") {
                Some("") => AXRole::Presentation,
                _ => AXRole::Img,
            }
        }
        "input" => input_role(node),
        "select" => {
            // `multiple` or `size > 1` → listbox; otherwise combobox.
            let multiple = node.get_attr("multiple").is_some();
            let size_gt1 = node
                .get_attr("size")
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|n| n > 1);
            if multiple || size_gt1 {
                AXRole::ListBox
            } else {
                AXRole::ComboBox
            }
        }
        "textarea" => AXRole::TextBox,
        "option" => AXRole::Option,
        "output" => AXRole::Status,
        "progress" => AXRole::Progressbar,
        "meter" => AXRole::Meter,
        "dialog" => AXRole::Dialog,
        "menu" => AXRole::Menu,
        "menuitem" => AXRole::MenuItem,

        // ── Document root ─────────────────────────────────────────────────────
        "html" | "body" => AXRole::Document,

        // ── No meaningful role ────────────────────────────────────────────────
        _ => AXRole::Generic,
    }
}

fn input_role(node: &Node) -> AXRole {
    match node.input_type() {
        Some(InputType::Checkbox) => AXRole::Checkbox,
        Some(InputType::Radio) => AXRole::Radio,
        Some(
            InputType::Submit
            | InputType::Reset
            | InputType::Button
            | InputType::Image,
        ) => AXRole::Button,
        Some(InputType::Range) => AXRole::Slider,
        Some(InputType::Number) => AXRole::Spinbutton,
        // color picker is a combobox/dialog; simplified to generic for now.
        Some(InputType::Color) => AXRole::ComboBox,
        // Hidden inputs are not in the AX tree.
        Some(InputType::Hidden) => AXRole::Presentation,
        // Search input is explicitly searchbox.
        Some(InputType::Search) => AXRole::Searchbox,
        // text, email, password, tel, url, date, time, … → textbox.
        _ => AXRole::TextBox,
    }
}
