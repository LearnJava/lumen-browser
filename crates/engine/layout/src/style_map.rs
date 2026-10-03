//! The published computed-style entry of one element (BUG-935 срез 69).
//!
//! `getComputedStyle()` reads a snapshot: node index → property → CSS text. An entry used
//! to be a `HashMap<String, String>` of ~200 properties built from scratch for every box —
//! ~400 allocations, ~70 µs on the engine thread, and 94 % of the computed-style collector
//! on `lenta.ru` (≈2.6 s over a minute of flushes). Only a handful of those properties
//! depend on where the box sits (the used `width`/`height`/`padding-*`/`margin-*`/insets
//! [`apply_used_geometry`] writes); the rest is a pure function of the cascaded style.
//!
//! So an entry is a shared `base` — the serialised style, one per [`ComputedStyle`]
//! allocation, shared by every box that holds that `Arc` and by every later flush that
//! finds it unchanged — plus a short list of `over`rides for the geometry. Building an
//! entry for a box whose style is already serialised costs the overrides (a dozen
//! allocations) instead of the whole map, and the published snapshot stops holding one
//! 25 KB map per node.
//!
//! [`apply_used_geometry`]: crate::resolved_geometry::apply_used_geometry

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::style::ComputedStyle;

/// One element's property → CSS text map: a shared base and its own overrides.
///
/// Reads see the overrides first. There is no removal — nothing publishes a property and
/// then retracts it.
#[derive(Clone, Default)]
pub struct StyleMap {
    base: Arc<HashMap<String, String>>,
    over: Vec<(Cow<'static, str>, String)>,
}

impl std::fmt::Debug for StyleMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl PartialEq for StyleMap {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl From<HashMap<String, String>> for StyleMap {
    fn from(map: HashMap<String, String>) -> Self {
        Self { base: Arc::new(map), over: Vec::new() }
    }
}

impl FromIterator<(String, String)> for StyleMap {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        iter.into_iter().collect::<HashMap<_, _>>().into()
    }
}

impl StyleMap {
    /// The entry whose base is the serialisation of `style`, shared with every other
    /// holder of the same `Arc`.
    pub(crate) fn of_style(style: &Arc<ComputedStyle>) -> Self {
        Self { base: shared_base(style), over: Vec::new() }
    }

    /// The value of `key`; an override shadows the base.
    pub fn get(&self, key: &str) -> Option<&String> {
        self.over.iter().find(|(k, _)| k == key).map(|(_, v)| v).or_else(|| self.base.get(key))
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Sets `key`, returning the value it had.
    pub fn insert(&mut self, key: impl Into<Cow<'static, str>>, value: String) -> Option<String> {
        let key = key.into();
        if let Some(slot) = self.over.iter_mut().find(|(k, _)| *k == key) {
            return Some(std::mem::replace(&mut slot.1, value));
        }
        let before = self.base.get(key.as_ref()).cloned();
        self.over.push((key, value));
        before
    }

    /// Every property once, overrides included, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &String)> {
        let over = self.over.iter().map(|(k, v)| (k.as_ref(), v));
        let base = self
            .base
            .iter()
            .filter(|(k, _)| !self.over.iter().any(|(o, _)| o == k.as_str()))
            .map(|(k, v)| (k.as_str(), v));
        over.chain(base)
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.over.is_empty() && self.base.is_empty()
    }
}

/// Styles whose serialisation is kept, by the address of the `Arc` that holds them. The
/// entry pins the `Arc`, so the address cannot be taken by another style while it lives,
/// and `Arc::make_mut` in a layout pass copies a style the memo has serialised instead of
/// changing it under the memo.
const MEMO_CAP: usize = 4096;

type Memo = HashMap<usize, (Arc<ComputedStyle>, Arc<HashMap<String, String>>)>;

/// `LUMEN_NO_SHARED_STYLE_BASE=1` serialises every style afresh and shares nothing, as
/// before the entry was split (rollback and A/B lever).
fn sharing() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| !std::env::var_os("LUMEN_NO_SHARED_STYLE_BASE").is_some_and(|v| v != "0"))
}

fn shared_base(style: &Arc<ComputedStyle>) -> Arc<HashMap<String, String>> {
    if !sharing() {
        return Arc::new(crate::selector_query::computed_style_to_map(style));
    }
    static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();
    let memo = MEMO.get_or_init(Mutex::default);
    let key = Arc::as_ptr(style) as usize;
    if let Some((_, base)) = memo.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return Arc::clone(base);
    }
    let built = Arc::new(crate::selector_query::computed_style_to_map(style));
    let mut m = memo.lock().unwrap_or_else(PoisonError::into_inner);
    if m.len() >= MEMO_CAP {
        // A style only the memo still holds is dead: no box, no cascade cache has it.
        m.retain(|_, (s, _)| Arc::strong_count(s) > 1);
        if m.len() >= MEMO_CAP {
            m.clear();
        }
    }
    m.insert(key, (Arc::clone(style), Arc::clone(&built)));
    built
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> StyleMap {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn an_override_shadows_the_base_and_reports_what_it_displaced() {
        let mut m = map(&[("width", "auto"), ("color", "red")]);
        assert_eq!(m.insert("width", "50px".to_owned()), Some("auto".to_owned()));
        assert_eq!(m.get("width").map(String::as_str), Some("50px"));
        assert_eq!(m.insert("width", "60px".to_owned()), Some("50px".to_owned()));
        assert_eq!(m.insert("left", "1px".to_owned()), None);
        assert_eq!(m.get("color").map(String::as_str), Some("red"));
        assert_eq!(m.get("top"), None);
    }

    #[test]
    fn iteration_lists_each_property_once_with_its_overridden_value() {
        let mut m = map(&[("width", "auto"), ("color", "red")]);
        m.insert("width", "50px".to_owned());
        m.insert("left", "1px".to_owned());
        let mut seen: Vec<_> = m.iter().map(|(k, v)| (k.to_owned(), v.clone())).collect();
        seen.sort();
        assert_eq!(
            seen,
            [("color", "red"), ("left", "1px"), ("width", "50px")].map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn maps_with_the_same_properties_are_equal_however_they_are_split() {
        let mut split = map(&[("width", "auto"), ("color", "red")]);
        split.insert("width", "50px".to_owned());
        assert_eq!(split, map(&[("width", "50px"), ("color", "red")]));
        assert_ne!(split, map(&[("width", "auto"), ("color", "red")]));
    }

    #[test]
    fn boxes_holding_one_style_arc_share_one_serialised_base() {
        let style = Arc::new(ComputedStyle::root());
        let (a, b) = (StyleMap::of_style(&style), StyleMap::of_style(&style));
        assert!(Arc::ptr_eq(&a.base, &b.base));
        let other = Arc::new(ComputedStyle::root());
        assert!(!Arc::ptr_eq(&a.base, &StyleMap::of_style(&other).base));
        assert_eq!(a, b);
    }
}
