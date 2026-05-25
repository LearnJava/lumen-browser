/// In-memory Web Storage partition (localStorage or sessionStorage).
///
/// Maintains insertion order for `key(index)` per the Web Storage spec
/// (HTML Living Standard §8.1).  Both key and value are DOMString (UTF-16 in
/// spec, Rust uses equivalent UTF-8 `String`).
///
/// Thread-safe via `Arc<Mutex<WebStorage>>`. The shell stores one
/// `Arc<Mutex<WebStorage>>` per origin for `localStorage` (persists across
/// page reloads within the session); a fresh instance is created per page load
/// for `sessionStorage`.
#[derive(Default, Clone)]
pub struct WebStorage {
    keys: Vec<String>,
    data: std::collections::HashMap<String, String>,
}

impl WebStorage {
    /// Number of stored key-value pairs.
    pub fn len(&self) -> u32 {
        self.keys.len() as u32
    }

    /// Returns `true` if the storage contains no items.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Return the nth key in insertion order, or `None` if out of range.
    pub fn key(&self, index: u32) -> Option<&str> {
        self.keys.get(index as usize).map(|s| s.as_str())
    }

    /// Return the value for `key`, or `None` if absent.
    pub fn get_item(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(|s| s.as_str())
    }

    /// Set `key` to `value`.  New keys are appended in insertion order.
    pub fn set_item(&mut self, key: String, value: String) {
        if !self.data.contains_key(&key) {
            self.keys.push(key.clone());
        }
        self.data.insert(key, value);
    }

    /// Remove `key` and its value.  No-op if absent.
    pub fn remove_item(&mut self, key: &str) {
        if self.data.remove(key).is_some() {
            self.keys.retain(|k| k != key);
        }
    }

    /// Remove all key-value pairs.
    pub fn clear(&mut self) {
        self.keys.clear();
        self.data.clear();
    }
}
