//! Session file export / import — portable JSON format (§12.7 / 2C).
//!
//! Файл сессии (.lsession) — переносимый JSON с набором вкладок.
//! Не зависит от SQLite (`tab_sessions`) — можно передавать между
//! компьютерами, делать бэкап, шарить коллеге.
//!
//! Формат v1:
//! ```json
//! {
//!   "created_at": 1716312345,
//!   "name": "auto-save",
//!   "tabs": [
//!     {"is_active":true,"scroll_x":0,"scroll_y":150,
//!      "title":"Example","url":"https://example.com/"}
//!   ],
//!   "version": 1
//! }
//! ```

use std::collections::BTreeMap;

use lumen_core::json::{parse as parse_json, JsonValue};

/// Portable session file structure.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionFile {
    /// Format version — always 1 for now.
    pub version: u32,
    /// Human-readable session name (e.g. "auto-save 2026-05-21").
    pub name: String,
    /// Unix timestamp (seconds) when the session was saved.
    pub created_at: i64,
    pub tabs: Vec<ExportedTab>,
}

/// One tab in a portable session file.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportedTab {
    pub url: String,
    pub title: String,
    /// Horizontal scroll in CSS px.
    pub scroll_x: f32,
    /// Vertical scroll in CSS px.
    pub scroll_y: f32,
    /// Whether this was the focused tab at export time.
    pub is_active: bool,
}

/// Serialize a [`SessionFile`] to a compact JSON string.
#[must_use]
pub fn to_json(file: &SessionFile) -> String {
    let tabs: Vec<JsonValue> = file
        .tabs
        .iter()
        .map(|t| {
            let mut obj = BTreeMap::new();
            obj.insert("is_active".into(), JsonValue::Bool(t.is_active));
            obj.insert("scroll_x".into(), JsonValue::Number(t.scroll_x as f64));
            obj.insert("scroll_y".into(), JsonValue::Number(t.scroll_y as f64));
            obj.insert("title".into(), JsonValue::String(t.title.clone()));
            obj.insert("url".into(), JsonValue::String(t.url.clone()));
            JsonValue::Object(obj)
        })
        .collect();

    let mut obj = BTreeMap::new();
    obj.insert("created_at".into(), JsonValue::Number(file.created_at as f64));
    obj.insert("name".into(), JsonValue::String(file.name.clone()));
    obj.insert("tabs".into(), JsonValue::Array(tabs));
    obj.insert("version".into(), JsonValue::Number(file.version as f64));
    JsonValue::Object(obj).to_string()
}

/// Deserialize a [`SessionFile`] from a JSON string.
///
/// Returns `Err` with a human-readable message on any parse / validation failure.
pub fn from_json(s: &str) -> Result<SessionFile, String> {
    let val = parse_json(s).map_err(|e| format!("JSON parse error: {e}"))?;

    let version = val
        .get("version")
        .and_then(|v| v.as_number())
        .map(|n| n as u32)
        .ok_or("missing field: version")?;

    if version != 1 {
        return Err(format!("unsupported session version: {version} (expected 1)"));
    }

    let name = val
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let created_at = val
        .get("created_at")
        .and_then(|v| v.as_number())
        .map(|n| n as i64)
        .unwrap_or(0);

    let tabs_arr = val
        .get("tabs")
        .and_then(|v| v.as_array())
        .ok_or("missing field: tabs (expected array)")?;

    let mut tabs = Vec::with_capacity(tabs_arr.len());
    for (i, tv) in tabs_arr.iter().enumerate() {
        let url = tv
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("tab[{i}]: missing field: url"))?
            .to_string();
        let title = tv
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let scroll_x = tv
            .get("scroll_x")
            .and_then(|v| v.as_number())
            .unwrap_or(0.0) as f32;
        let scroll_y = tv
            .get("scroll_y")
            .and_then(|v| v.as_number())
            .unwrap_or(0.0) as f32;
        let is_active = tv
            .get("is_active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        tabs.push(ExportedTab { url, title, scroll_x, scroll_y, is_active });
    }

    Ok(SessionFile { version, name, created_at, tabs })
}

/// Return the first active tab, or the first tab if none is marked active.
#[must_use]
pub fn active_tab(file: &SessionFile) -> Option<&ExportedTab> {
    file.tabs.iter().find(|t| t.is_active).or_else(|| file.tabs.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file() -> SessionFile {
        SessionFile {
            version: 1,
            name: "test-session".into(),
            created_at: 1_716_312_345,
            tabs: vec![
                ExportedTab {
                    url: "https://example.com/".into(),
                    title: "Example".into(),
                    scroll_x: 0.0,
                    scroll_y: 150.0,
                    is_active: true,
                },
                ExportedTab {
                    url: "https://rust-lang.org/".into(),
                    title: "Rust".into(),
                    scroll_x: 0.0,
                    scroll_y: 0.0,
                    is_active: false,
                },
            ],
        }
    }

    #[test]
    fn roundtrip_preserves_all_fields() {
        let original = sample_file();
        let json = to_json(&original);
        let parsed = from_json(&json).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.name, "test-session");
        assert_eq!(parsed.created_at, 1_716_312_345);
        assert_eq!(parsed.tabs.len(), 2);
        assert_eq!(parsed.tabs[0].url, "https://example.com/");
        assert_eq!(parsed.tabs[0].title, "Example");
        assert_eq!(parsed.tabs[0].scroll_y, 150.0);
        assert!(parsed.tabs[0].is_active);
        assert!(!parsed.tabs[1].is_active);
    }

    #[test]
    fn empty_tabs_allowed() {
        let file = SessionFile {
            version: 1,
            name: "empty".into(),
            created_at: 0,
            tabs: vec![],
        };
        let parsed = from_json(&to_json(&file)).unwrap();
        assert!(parsed.tabs.is_empty());
    }

    #[test]
    fn cyrillic_url_and_title() {
        let file = SessionFile {
            version: 1,
            name: "кириллица".into(),
            created_at: 0,
            tabs: vec![ExportedTab {
                url: "https://пример.рф/".into(),
                title: "Главная страница".into(),
                scroll_x: 0.0,
                scroll_y: 0.0,
                is_active: true,
            }],
        };
        let parsed = from_json(&to_json(&file)).unwrap();
        assert_eq!(parsed.tabs[0].url, "https://пример.рф/");
        assert_eq!(parsed.tabs[0].title, "Главная страница");
        assert_eq!(parsed.name, "кириллица");
    }

    #[test]
    fn wrong_version_returns_error() {
        let json = r#"{"created_at":0,"name":"x","tabs":[],"version":2}"#;
        let err = from_json(json).unwrap_err();
        assert!(err.contains("unsupported session version: 2"), "got: {err}");
    }

    #[test]
    fn missing_version_returns_error() {
        let json = r#"{"created_at":0,"name":"x","tabs":[]}"#;
        assert!(from_json(json).is_err());
    }

    #[test]
    fn missing_url_in_tab_returns_error() {
        let json = r#"{"created_at":0,"name":"x","tabs":[{"title":"t"}],"version":1}"#;
        let err = from_json(json).unwrap_err();
        assert!(err.contains("url"), "got: {err}");
    }

    #[test]
    fn invalid_json_returns_error() {
        assert!(from_json("not json").is_err());
    }

    #[test]
    fn active_tab_returns_first_active() {
        let file = sample_file();
        let tab = active_tab(&file).unwrap();
        assert_eq!(tab.url, "https://example.com/");
    }

    #[test]
    fn active_tab_falls_back_to_first_if_none_active() {
        let mut file = sample_file();
        for t in &mut file.tabs {
            t.is_active = false;
        }
        let tab = active_tab(&file).unwrap();
        assert_eq!(tab.url, "https://example.com/");
    }

    #[test]
    fn active_tab_returns_none_for_empty() {
        let file = SessionFile { version: 1, name: String::new(), created_at: 0, tabs: vec![] };
        assert!(active_tab(&file).is_none());
    }

    #[test]
    fn json_contains_sorted_keys() {
        // BTreeMap → ключи в алфавитном порядке — детерминированный вывод
        let json = to_json(&sample_file());
        let created = json.find("created_at").unwrap_or(usize::MAX);
        let name_pos = json.find(r#""name""#).unwrap_or(usize::MAX);
        let tabs_pos = json.find(r#""tabs""#).unwrap_or(usize::MAX);
        let version_pos = json.find(r#""version""#).unwrap_or(usize::MAX);
        assert!(created < name_pos && name_pos < tabs_pos && tabs_pos < version_pos);
    }

    #[test]
    fn optional_fields_default_to_zero_when_absent() {
        // scroll_x/scroll_y/is_active are optional in the format
        let json = r#"{"created_at":0,"name":"x","tabs":[{"url":"https://a/"}],"version":1}"#;
        let file = from_json(json).unwrap();
        assert_eq!(file.tabs[0].scroll_x, 0.0);
        assert_eq!(file.tabs[0].scroll_y, 0.0);
        assert!(!file.tabs[0].is_active);
    }
}
