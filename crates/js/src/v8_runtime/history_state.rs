//! Стек `history` страницы для JS-рантайма — приватное зеркало
//! `dom::HistoryState`.
//!
//! Вынесено из `v8_runtime.rs` батчем SPLIT-JS5; см. соседний
//! [`super::dom_helpers`] — там лежит остальная семья приватных копий
//! помощников `dom.rs` (S3), объявленная тем же баннером.

/// Mirrors `dom::HistoryState` (private there) — per-page JS `history` stack.
pub(super) struct HistoryState {
    entries: Vec<(String, String)>,
    current: usize,
}

impl HistoryState {
    pub(super) fn new() -> Self {
        Self {
            entries: vec![(String::from("null"), String::new())],
            current: 0,
        }
    }

    pub(super) fn push(&mut self, state_json: String, url: String) {
        self.entries.truncate(self.current + 1);
        self.entries.push((state_json, url));
        self.current = self.entries.len() - 1;
    }

    pub(super) fn replace(&mut self, state_json: String, url: String) {
        if let Some(e) = self.entries.get_mut(self.current) {
            *e = (state_json, url);
        }
    }

    pub(super) fn set_state(&mut self, state_json: String) {
        if let Some(e) = self.entries.get_mut(self.current) {
            e.0 = state_json;
        }
    }

    pub(super) fn go(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let new_idx = self.current as i64 + i64::from(delta);
        if new_idx < 0 || new_idx >= self.entries.len() as i64 {
            return false;
        }
        self.current = new_idx as usize;
        true
    }

    pub(super) fn state_json(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.0.as_str())
            .unwrap_or("null")
    }

    pub(super) fn url(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.1.as_str())
            .unwrap_or("")
    }

    pub(super) fn length(&self) -> u32 {
        self.entries.len() as u32
    }
}
