//! Which `<style>`/`<link rel=stylesheet>` owners take part in the cascade (BUG-1554).
//!
//! HTML LS §4.2.4 «alternative style sheet sets» plus the `disabled` knobs a script has:
//! `HTMLLinkElement.disabled`, `CSSStyleSheet.disabled`, `HTMLStyleElement.disabled`. Layout of
//! the answer lives here, on the `Document`, because two crates need the same verdict — the JS
//! runtime (`document.styleSheets`, `getComputedStyle`) and the shell (the cascade it paints).

use crate::{Document, NodeData, NodeId};

/// What a script said about one owner, overriding the markup-derived default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SheetOverride {
    /// `sheet.disabled = false` / `link.disabled = false` on a present attribute: the sheet
    /// applies even if it is an alternate or not in the preferred set.
    Enabled,
    /// `sheet.disabled = true`: the sheet does not apply.
    Disabled,
}

fn rel_tokens(rel: &str) -> impl Iterator<Item = &str> {
    rel.split_ascii_whitespace()
}

impl Document {
    /// Script verdict recorded for `id` by [`Document::set_sheet_override`].
    pub fn sheet_override(&self, id: NodeId) -> Option<SheetOverride> {
        self.sheet_overrides.get(&id).copied()
    }

    /// Record (`Some`) or forget (`None`) a script verdict for `id`. Keyed by node, so it survives
    /// a remove/re-insert of the same element and is not copied by `cloneNode`.
    pub fn set_sheet_override(&mut self, id: NodeId, state: Option<SheetOverride>) {
        match state {
            Some(s) => self.sheet_overrides.insert(id, s),
            None => self.sheet_overrides.remove(&id),
        };
    }

    /// The «preferred style sheet set»: `<meta http-equiv=default-style content=X>` if present,
    /// else the title of the first non-alternate, titled `<style>`/`<link rel=stylesheet>`
    /// **created** — the smallest node id, not the first in tree order: browsers fix the set when
    /// the first such sheet appears, and a sheet script put earlier in the tree later does not
    /// take it over (WPT `preferred-stylesheet-reversed-order`). `None` — every titled sheet is
    /// then an alternate.
    pub fn preferred_sheet_set(&self) -> Option<String> {
        let mut first_titled: Option<(usize, String)> = None;
        let mut meta = None;
        self.walk_sheet_candidates(self.root(), &mut |doc, id| {
            let node = doc.get(id);
            let Some(name) = node.element_name() else { return };
            match name.local.as_str() {
                "meta" => {
                    if meta.is_none()
                        && node.get_attr("http-equiv").is_some_and(|v| v.eq_ignore_ascii_case("default-style"))
                        && let Some(c) = node.get_attr("content").filter(|c| !c.is_empty())
                    {
                        meta = Some(c.to_owned());
                    }
                }
                "style" | "link" => {
                    if name.local == "link" {
                        let rel = node.get_attr("rel").unwrap_or("");
                        let is_sheet = rel_tokens(rel).any(|t| t.eq_ignore_ascii_case("stylesheet"));
                        if !is_sheet || Self::link_is_alternate(rel) {
                            return;
                        }
                    }
                    if first_titled.as_ref().is_some_and(|(n, _)| *n <= id.index()) {
                        return;
                    }
                    if let Some(t) = node.get_attr("title").filter(|t| !t.is_empty()) {
                        first_titled = Some((id.index(), t.to_owned()));
                    }
                }
                _ => {}
            }
        });
        meta.or(first_titled.map(|(_, t)| t))
    }

    fn link_is_alternate(rel: &str) -> bool {
        rel_tokens(rel).any(|t| t.eq_ignore_ascii_case("alternate"))
    }

    fn walk_sheet_candidates(&self, id: NodeId, f: &mut dyn FnMut(&Document, NodeId)) {
        let node = self.get(id);
        if matches!(node.data, NodeData::Element { .. }) {
            f(self, id);
        }
        for &c in &node.children {
            self.walk_sheet_candidates(c, f);
        }
    }

    /// Does the owner `id` (a `<style>` or a `<link>`) apply, given the document's `preferred`
    /// set? A script verdict wins; otherwise the markup decides: an untitled non-alternate sheet
    /// always applies, a titled or alternate one only when its title is the preferred set.
    pub fn sheet_owner_enabled(&self, id: NodeId, preferred: Option<&str>) -> bool {
        match self.sheet_override(id) {
            Some(SheetOverride::Enabled) => return true,
            Some(SheetOverride::Disabled) => return false,
            None => {}
        }
        let node = self.get(id);
        let title = node.get_attr("title").unwrap_or("");
        let alternate = node.element_name().is_some_and(|n| n.local == "link")
            && Self::link_is_alternate(node.get_attr("rel").unwrap_or(""));
        if title.is_empty() {
            return !alternate;
        }
        preferred == Some(title)
    }

    /// A `<link disabled>` is not a style sheet at all until the attribute goes away (HTML LS
    /// §4.6.7 «disabled»): it has no `.sheet` and is absent from `document.styleSheets`.
    pub fn link_has_no_sheet(&self, id: NodeId) -> bool {
        let node = self.get(id);
        node.element_name().is_some_and(|n| n.local == "link")
            && node.get_attr("disabled").is_some()
    }
}
