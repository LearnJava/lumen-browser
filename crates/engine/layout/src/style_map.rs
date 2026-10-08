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
//! BUG-935 срез 72: the base is serialised on the first *read*, not when the entry is built.
//! A page reads a handful of elements; the collector builds an entry for every element it
//! restyled, and on the first collect of a page (or a mass restyle) none of the styles is
//! serialised yet — 190 ms of `flush.collect_computed_styles` on `lenta.ru` for ~1 900 entries
//! nobody read.
//!
//! [`apply_used_geometry`]: crate::resolved_geometry::apply_used_geometry

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::resolved_geometry::COMPUTED_VALUE_KEY_PREFIX;
use crate::style::ComputedStyle;

/// The serialised style an entry is built on, produced from `style` on first use.
#[derive(Default)]
struct Base {
    style: Option<Arc<ComputedStyle>>,
    map: OnceLock<HashMap<String, String>>,
}

impl Base {
    fn lazy(style: &Arc<ComputedStyle>) -> Self {
        Self { style: Some(Arc::clone(style)), map: OnceLock::new() }
    }

    fn of_map(map: HashMap<String, String>) -> Self {
        let base = Self::default();
        let _ = base.map.set(map);
        base
    }

    fn map(&self) -> &HashMap<String, String> {
        self.map.get_or_init(|| match &self.style {
            Some(style) => crate::selector_query::computed_style_to_map(style),
            None => HashMap::new(),
        })
    }
}

/// One override: a plain property, or a *used* value (layout geometry) that displaced the
/// computed one, which stays reachable under [`COMPUTED_VALUE_KEY_PREFIX`].
#[derive(Clone)]
struct Over {
    key: Cow<'static, str>,
    value: String,
    used: bool,
}

/// One element's property → CSS text map: a shared base and its own overrides.
///
/// Reads see the overrides first. There is no removal — nothing publishes a property and
/// then retracts it.
#[derive(Clone, Default)]
pub struct StyleMap {
    base: Arc<Base>,
    over: Vec<Over>,
}

impl std::fmt::Debug for StyleMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.entries()).finish()
    }
}

impl PartialEq for StyleMap {
    fn eq(&self, other: &Self) -> bool {
        self.entries() == other.entries()
    }
}

impl From<HashMap<String, String>> for StyleMap {
    fn from(map: HashMap<String, String>) -> Self {
        Self { base: Arc::new(Base::of_map(map)), over: Vec::new() }
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

    /// The entry for a style that lives by value (an inline segment's), serialised from a copy
    /// on first read — not shared, but a clone is far cheaper than the serialisation.
    pub(crate) fn of_style_copy(style: &ComputedStyle) -> Self {
        Self { base: Arc::new(Base::lazy(&Arc::new(style.clone()))), over: Vec::new() }
    }

    /// The value of `key`; an override shadows the base.
    pub fn get(&self, key: &str) -> Option<&String> {
        if let Some(o) = self.over.iter().find(|o| o.key == key) {
            return Some(&o.value);
        }
        if let Some(name) = key.strip_prefix(COMPUTED_VALUE_KEY_PREFIX)
            && let Some(used) = self.over.iter().find(|o| o.used && o.key == name)
        {
            return self.base.map().get(name).filter(|computed| **computed != used.value);
        }
        self.base.map().get(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Sets `key`, returning the value it had.
    pub fn insert(&mut self, key: impl Into<Cow<'static, str>>, value: String) -> Option<String> {
        let key = key.into();
        if let Some(slot) = self.over.iter_mut().find(|o| o.key == key) {
            return Some(std::mem::replace(&mut slot.value, value));
        }
        let before = self.base.map().get(key.as_ref()).cloned();
        self.over.push(Over { key, value, used: false });
        before
    }

    /// Sets `key` without reading the base, so an entry nobody reads is never serialised.
    pub fn set(&mut self, key: impl Into<Cow<'static, str>>, value: String) {
        let key = key.into();
        match self.over.iter_mut().find(|o| o.key == key) {
            Some(slot) => slot.value = value,
            None => self.over.push(Over { key, value, used: false }),
        }
    }

    /// Sets the *used* value of `name`. The computed value it displaced is answered under
    /// [`COMPUTED_VALUE_KEY_PREFIX`]`+name` when the two differ — decided on that read, so
    /// setting a used value does not serialise the base.
    pub fn set_used(&mut self, name: &'static str, value: String) {
        match self.over.iter_mut().find(|o| o.key == name) {
            Some(slot) => {
                slot.value = value;
                slot.used = true;
            }
            None => self.over.push(Over { key: Cow::Borrowed(name), value, used: true }),
        }
    }

    /// Every property once, overrides included, in no particular order. The computed values a
    /// used one displaced (the `computed:`-prefixed keys) are reached through [`StyleMap::get`].
    pub fn iter(&self) -> impl Iterator<Item = (&str, &String)> {
        let over = self.over.iter().map(|o| (o.key.as_ref(), &o.value));
        let base = self
            .base
            .map()
            .iter()
            .filter(|(k, _)| !self.over.iter().any(|o| o.key == k.as_str()))
            .map(|(k, v)| (k.as_str(), v));
        over.chain(base)
    }

    /// `iter()` plus the displaced computed values, keyed as `get` answers them.
    fn entries(&self) -> BTreeMap<String, &String> {
        let displaced = self.over.iter().filter(|o| o.used).filter_map(|o| {
            let name = o.key.as_ref();
            self.base
                .map()
                .get(name)
                .filter(|computed| **computed != o.value)
                .map(|computed| (format!("{COMPUTED_VALUE_KEY_PREFIX}{name}"), computed))
        });
        self.iter().map(|(k, v)| (k.to_owned(), v)).chain(displaced).collect()
    }

    /// Whether the base has been serialised yet (tests: the collector must leave it lazy).
    #[cfg(test)]
    pub(crate) fn is_serialised(&self) -> bool {
        self.base.map.get().is_some()
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.over.is_empty() && self.base.map().is_empty()
    }
}

/// Styles whose serialisation is kept, by the address of the `Arc` that holds them. The
/// entry pins the `Arc` (through its [`Base`]), so the address cannot be taken by another
/// style while it lives, and `Arc::make_mut` in a layout pass copies a style the memo has
/// serialised instead of changing it under the memo.
const MEMO_CAP: usize = 4096;

type Memo = HashMap<usize, Arc<Base>>;

/// `LUMEN_NO_SHARED_STYLE_BASE=1` serialises every style afresh and shares nothing, as
/// before the entry was split (rollback and A/B lever).
fn sharing() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| !std::env::var_os("LUMEN_NO_SHARED_STYLE_BASE").is_some_and(|v| v != "0"))
}

fn shared_base(style: &Arc<ComputedStyle>) -> Arc<Base> {
    if !sharing() {
        return Arc::new(Base::of_map(crate::selector_query::computed_style_to_map(style)));
    }
    static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();
    let memo = MEMO.get_or_init(Mutex::default);
    let key = Arc::as_ptr(style) as usize;
    let mut m = memo.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(base) = m.get(&key) {
        return Arc::clone(base);
    }
    if m.len() >= MEMO_CAP {
        // A style only the memo's base still holds is dead: no box, no cascade cache has it.
        m.retain(|_, base| base.style.as_ref().is_some_and(|s| Arc::strong_count(s) > 1));
        if m.len() >= MEMO_CAP {
            m.clear();
        }
    }
    let base = Arc::new(Base::lazy(style));
    m.insert(key, Arc::clone(&base));
    base
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

    #[test]
    fn a_used_value_keeps_the_computed_one_under_the_prefix_only_when_they_differ() {
        let mut m = map(&[("width", "auto"), ("height", "10px")]);
        m.set_used("width", "50px".to_owned());
        m.set_used("height", "10px".to_owned());
        assert_eq!(m.get("width").map(String::as_str), Some("50px"));
        assert_eq!(m.get("computed:width").map(String::as_str), Some("auto"));
        assert_eq!(m.get("height").map(String::as_str), Some("10px"));
        assert_eq!(m.get("computed:height"), None);
        assert_eq!(m.get("computed:color"), None);
    }

    #[test]
    fn a_used_value_without_a_computed_one_has_nothing_to_stash() {
        let mut m = map(&[("color", "red")]);
        m.set_used("left", "3px".to_owned());
        assert_eq!(m.get("left").map(String::as_str), Some("3px"));
        assert_eq!(m.get("computed:left"), None);
    }

    #[test]
    fn building_an_entry_does_not_serialise_the_style() {
        let style = Arc::new(ComputedStyle::root());
        let mut m = StyleMap::of_style(&style);
        m.set_used("width", "50px".to_owned());
        m.set("computed:-lumen-boxless", "1".to_owned());
        assert!(m.base.map.get().is_none(), "entry built without being read");
        assert_eq!(m.get("width").map(String::as_str), Some("50px"));
        assert!(m.base.map.get().is_none(), "a used override is answered without the base");
        assert!(m.get("display").is_some());
        assert!(m.base.map.get().is_some());
    }
}
