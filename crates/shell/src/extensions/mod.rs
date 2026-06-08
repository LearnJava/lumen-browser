//! Browser extension system stub (D-6).
//!
//! Phase 0: loads extensions from `<config>/lumen/extensions/<id>/manifest.json`,
//! injects matching `content_scripts` into each page as extra scripts, and
//! provides a `chrome.runtime.sendMessage()` stub so existing extension code
//! does not throw on import.
//!
//! Directory layout expected on disk:
//! ```text
//! <config>/lumen/extensions/
//!   my-ext/
//!     manifest.json
//!     content.js
//!   another-ext/
//!     manifest.json
//!     inject.js
//! ```
//!
//! Manifest format (Chrome MV3 subset, JSON):
//! ```json
//! {
//!   "name": "My Extension",
//!   "version": "1.0",
//!   "permissions": ["storage"],
//!   "content_scripts": [{"matches": ["https://example.com/*"], "js": ["content.js"]}]
//! }
//! ```

use std::path::{Path, PathBuf};

/// A single content-script entry from `manifest.json`.
#[derive(Debug, Clone)]
pub struct ContentScript {
    /// URL match patterns (Chrome-style glob: `*://example.com/*`).
    pub matches: Vec<String>,
    /// JS file names relative to the extension directory.
    pub js: Vec<String>,
}

/// A parsed `manifest.json` for one extension.
#[derive(Debug, Clone)]
pub struct ExtensionManifest {
    /// Human-readable extension name.
    #[allow(dead_code)]
    pub name: String,
    /// Extension version string.
    #[allow(dead_code)]
    pub version: String,
    /// Declared permissions (informational in Phase 0, not enforced).
    #[allow(dead_code)]
    pub permissions: Vec<String>,
    /// Content scripts to inject into matching pages.
    pub content_scripts: Vec<ContentScript>,
}

/// One successfully loaded extension: manifest + directory on disk.
#[derive(Debug, Clone)]
struct LoadedExtension {
    manifest: ExtensionManifest,
    /// Directory that contains `manifest.json`; JS paths are resolved relative to it.
    dir: PathBuf,
}

/// Registry of all installed extensions for the current profile.
///
/// Created once at browser startup via [`ExtensionRegistry::load`], then queried
/// per page-load via [`ExtensionRegistry::content_scripts_for_url`].
#[derive(Debug, Default)]
pub struct ExtensionRegistry {
    extensions: Vec<LoadedExtension>,
}

/// Return the extensions directory for the current profile.
///
/// - Windows: `%APPDATA%\lumen\extensions\`
/// - other:   `$XDG_CONFIG_HOME/lumen/extensions/` or `~/.config/lumen/extensions/`
///
/// Returns `None` only when none of the required environment variables are set.
#[must_use]
pub fn extensions_dir() -> Option<PathBuf> {
    let base: PathBuf = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))?
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
    };
    Some(base.join("lumen").join("extensions"))
}

impl ExtensionRegistry {
    /// Scan the extensions directory and load all valid extensions.
    ///
    /// Silently skips entries that are not directories, have no `manifest.json`,
    /// or have an unparseable manifest. Never panics.
    #[must_use]
    pub fn load() -> Self {
        let Some(dir) = extensions_dir() else {
            return Self::default();
        };
        Self::load_from_dir(&dir)
    }

    /// Load extensions from an explicit directory (used in tests).
    #[must_use]
    pub fn load_from_dir(dir: &Path) -> Self {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Self::default();
        };
        let mut extensions = Vec::new();
        for entry in entries.flatten() {
            let ext_dir = entry.path();
            if !ext_dir.is_dir() {
                continue;
            }
            let manifest_path = ext_dir.join("manifest.json");
            let Ok(json) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            if let Some(manifest) = parse_manifest(&json) {
                extensions.push(LoadedExtension {
                    manifest,
                    dir: ext_dir,
                });
            }
        }
        Self { extensions }
    }

    /// Return the number of loaded extensions.
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.extensions.len()
    }

    /// Return `true` if no extensions are loaded.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    /// Collect all JS source strings for content scripts that match `page_url`.
    ///
    /// Reads the JS files from disk at call time. Missing or unreadable files are
    /// silently skipped. Returns an empty vec when no extension matches.
    #[must_use]
    pub fn content_scripts_for_url(&self, page_url: &str) -> Vec<String> {
        let mut scripts = Vec::new();
        for ext in &self.extensions {
            for cs in &ext.manifest.content_scripts {
                if cs.matches.iter().any(|pat| url_matches(page_url, pat)) {
                    for js_file in &cs.js {
                        let path = ext.dir.join(js_file);
                        if let Ok(src) = std::fs::read_to_string(&path) {
                            scripts.push(src);
                        }
                    }
                }
            }
        }
        scripts
    }
}

// ─── manifest parser (hand-rolled; no serde dependency) ──────────────────────

/// Parse a Chrome-style extension `manifest.json` (subset).
///
/// Uses a simple hand-rolled JSON scanner — avoids adding serde as a dependency.
/// Returns `None` when the manifest is missing required fields (`name`/`version`).
fn parse_manifest(json: &str) -> Option<ExtensionManifest> {
    let name = extract_string(json, "name")?;
    let version = extract_string(json, "version")?;
    let permissions = extract_string_array(json, "permissions");
    let content_scripts = parse_content_scripts(json);
    Some(ExtensionManifest { name, version, permissions, content_scripts })
}

/// Extract a top-level `"key": "value"` string from JSON.
fn extract_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(needle.as_str())?;
    let rest = &json[pos + needle.len()..];
    let colon_pos = rest.find(':')?;
    let after_colon = rest[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let content = &after_colon[1..];
    let end = content.find('"')?;
    Some(content[..end].to_string())
}

/// Extract a top-level `"key": ["a", "b", ...]` string array from JSON.
fn extract_string_array(json: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{}\"", key);
    let Some(pos) = json.find(needle.as_str()) else {
        return Vec::new();
    };
    let rest = &json[pos + needle.len()..];
    let Some(colon_pos) = rest.find(':') else {
        return Vec::new();
    };
    let after_colon = rest[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('[') {
        return Vec::new();
    }
    let Some(close) = after_colon.find(']') else {
        return Vec::new();
    };
    collect_strings_from_array(&after_colon[..=close])
}

/// Collect all quoted strings from a JSON array literal `["a","b",...]`.
fn collect_strings_from_array(array_str: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut rest = array_str;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else {
            break;
        };
        result.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    result
}

/// Parse `content_scripts` array from the manifest JSON.
///
/// Each entry is `{"matches": [...], "js": [...]}`.  The parser finds the
/// `"content_scripts"` key and then scans object boundaries by bracket depth.
fn parse_content_scripts(json: &str) -> Vec<ContentScript> {
    let Some(cs_pos) = json.find("\"content_scripts\"") else {
        return Vec::new();
    };
    let rest = &json[cs_pos + "\"content_scripts\"".len()..];
    let Some(colon_pos) = rest.find(':') else {
        return Vec::new();
    };
    let after_colon = rest[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('[') {
        return Vec::new();
    }

    // Find the matching closing bracket of the outer array.
    let outer_end = find_matching_bracket(after_colon, '[', ']');
    let array_body = &after_colon[1..outer_end];

    let mut scripts = Vec::new();
    let mut depth = 0i32;
    let mut obj_start: Option<usize> = None;

    for (i, ch) in array_body.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    obj_start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0
                    && let Some(start) = obj_start.take()
                {
                    let obj = &array_body[start..=i];
                    let matches = extract_string_array(obj, "matches");
                    let js = extract_string_array(obj, "js");
                    if !matches.is_empty() || !js.is_empty() {
                        scripts.push(ContentScript { matches, js });
                    }
                }
            }
            _ => {}
        }
    }
    scripts
}

/// Find the index of the character that closes the bracket opened at index 0.
///
/// `open`/`close` are the bracket characters (e.g. `'['`/`']'` or `'{'`/`'}'`).
/// Returns `input.len()` if no matching close is found.
fn find_matching_bracket(input: &str, open: char, close: char) -> usize {
    let mut depth = 0i32;
    for (i, ch) in input.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    input.len()
}

// ─── URL pattern matching ─────────────────────────────────────────────────────

/// Match `url` against a Chrome-style content-script match pattern.
///
/// Supported forms (per Chrome Extensions docs §match patterns):
/// - `"<all_urls>"` — matches everything
/// - `"*://example.com/*"` — scheme wildcard + host + path glob
/// - `"https://example.com/*"` — explicit scheme + host + path glob
///
/// Path component supports only `*` (matches any sequence of chars including `/`).
/// Host component `*` matches any host. `*.example.com` matches subdomains.
/// Returns `false` for any malformed pattern.
pub fn url_matches(url: &str, pattern: &str) -> bool {
    if pattern == "<all_urls>" {
        return true;
    }

    // Split pattern at "://"
    let Some(scheme_end) = pattern.find("://") else {
        return false;
    };
    let pat_scheme = &pattern[..scheme_end];
    let pat_rest = &pattern[scheme_end + 3..];

    // Split url at "://"
    let Some(url_scheme_end) = url.find("://") else {
        return false;
    };
    let url_scheme = &url[..url_scheme_end];
    let url_rest = &url[url_scheme_end + 3..];

    // Scheme match: "*" matches any scheme
    if pat_scheme != "*" && pat_scheme != url_scheme {
        return false;
    }

    // Split host and path on first '/'
    let (pat_host, pat_path) = match pat_rest.find('/') {
        Some(i) => (&pat_rest[..i], &pat_rest[i..]),
        None => (pat_rest, "/"),
    };
    let (url_host, url_path) = match url_rest.find('/') {
        Some(i) => (&url_rest[..i], &url_rest[i..]),
        None => (url_rest, "/"),
    };

    // Host match
    if !host_matches(url_host, pat_host) {
        return false;
    }

    // Path match (glob with '*')
    glob_match(url_path, pat_path)
}

/// Match host `url_host` against pattern host `pat_host`.
///
/// `*` in pattern matches any host. `*.example.com` matches `sub.example.com`
/// and `example.com` itself. Otherwise exact match (case-insensitive).
fn host_matches(url_host: &str, pat_host: &str) -> bool {
    if pat_host == "*" {
        return true;
    }
    if let Some(suffix) = pat_host.strip_prefix("*.") {
        // *.example.com matches sub.example.com AND example.com
        let url_lower = url_host.to_ascii_lowercase();
        let suf_lower = suffix.to_ascii_lowercase();
        return url_lower == suf_lower || url_lower.ends_with(&format!(".{suf_lower}"));
    }
    url_host.eq_ignore_ascii_case(pat_host)
}

/// Simple glob matching where `*` matches any sequence of characters.
fn glob_match(s: &str, pattern: &str) -> bool {
    // Split pattern on '*' and match greedily.
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return s.is_empty();
    }
    let mut rest = s;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            // First segment must match prefix
            if !rest.starts_with(part) {
                return false;
            }
            rest = &rest[part.len()..];
        } else if i == parts.len() - 1 {
            // Last segment must match suffix
            return rest.ends_with(part);
        } else {
            // Middle segments: find first occurrence
            let Some(pos) = rest.find(part) else {
                return false;
            };
            rest = &rest[pos + part.len()..];
        }
    }
    true
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a unique temp directory for the test and return its path.
    /// The caller is responsible for cleanup (or leaving it to the OS).
    fn make_test_dir(suffix: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("lumen-ext-test-{suffix}"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn make_ext_dir(parent: &Path, id: &str, manifest: &str, scripts: &[(&str, &str)]) {
        let ext_dir = parent.join(id);
        fs::create_dir_all(&ext_dir).unwrap();
        fs::write(ext_dir.join("manifest.json"), manifest).unwrap();
        for (name, src) in scripts {
            fs::write(ext_dir.join(name), src).unwrap();
        }
    }

    #[test]
    fn parse_manifest_basic() {
        let json = r#"{"name":"Test","version":"1.0","permissions":["storage"],"content_scripts":[]}"#;
        let m = parse_manifest(json).unwrap();
        assert_eq!(m.name, "Test");
        assert_eq!(m.version, "1.0");
        assert_eq!(m.permissions, vec!["storage"]);
        assert!(m.content_scripts.is_empty());
    }

    #[test]
    fn parse_manifest_with_content_scripts() {
        let json = r#"{
            "name": "Injector",
            "version": "2.0",
            "permissions": ["activeTab", "storage"],
            "content_scripts": [
                {"matches": ["https://example.com/*"], "js": ["inject.js"]}
            ]
        }"#;
        let m = parse_manifest(json).unwrap();
        assert_eq!(m.content_scripts.len(), 1);
        assert_eq!(m.content_scripts[0].matches, vec!["https://example.com/*"]);
        assert_eq!(m.content_scripts[0].js, vec!["inject.js"]);
    }

    #[test]
    fn parse_manifest_missing_name_returns_none() {
        let json = r#"{"version":"1.0"}"#;
        assert!(parse_manifest(json).is_none());
    }

    #[test]
    fn url_matches_all_urls() {
        assert!(url_matches("https://example.com/page", "<all_urls>"));
        assert!(url_matches("http://other.org/", "<all_urls>"));
    }

    #[test]
    fn url_matches_exact_host_and_path_glob() {
        assert!(url_matches("https://example.com/foo/bar", "https://example.com/*"));
        assert!(!url_matches("https://other.com/foo", "https://example.com/*"));
        assert!(!url_matches("http://example.com/foo", "https://example.com/*"));
    }

    #[test]
    fn url_matches_scheme_wildcard() {
        assert!(url_matches("https://example.com/", "*://example.com/*"));
        assert!(url_matches("http://example.com/", "*://example.com/*"));
        assert!(!url_matches("https://other.com/", "*://example.com/*"));
    }

    #[test]
    fn url_matches_subdomain_wildcard() {
        assert!(url_matches("https://sub.example.com/", "https://*.example.com/*"));
        assert!(url_matches("https://example.com/", "https://*.example.com/*"));
        assert!(!url_matches("https://other.com/", "https://*.example.com/*"));
    }

    #[test]
    fn registry_load_from_dir_reads_extensions() {
        let tmp = make_test_dir("load-ext");
        let manifest = r#"{"name":"Hi","version":"1","permissions":[],"content_scripts":[{"matches":["https://example.com/*"],"js":["hi.js"]}]}"#;
        make_ext_dir(&tmp, "hi-ext", manifest, &[("hi.js", "console.log('hi');")]);

        let reg = ExtensionRegistry::load_from_dir(&tmp);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn registry_content_scripts_for_url_returns_source() {
        let tmp = make_test_dir("scripts-url");
        let manifest = r#"{"name":"Hi","version":"1","permissions":[],"content_scripts":[{"matches":["https://example.com/*"],"js":["hi.js"]}]}"#;
        make_ext_dir(&tmp, "hi-ext", manifest, &[("hi.js", "var x=1;")]);

        let reg = ExtensionRegistry::load_from_dir(&tmp);
        let scripts = reg.content_scripts_for_url("https://example.com/page");
        assert_eq!(scripts, vec!["var x=1;"]);

        let none = reg.content_scripts_for_url("https://other.com/page");
        assert!(none.is_empty());
    }

    #[test]
    fn registry_empty_when_dir_missing() {
        let reg = ExtensionRegistry::load_from_dir(Path::new(
            "/nonexistent/lumen-ext-test-missing-xyz",
        ));
        assert!(reg.is_empty());
    }

    #[test]
    fn registry_skips_invalid_manifests() {
        let tmp = make_test_dir("skip-invalid");
        let good = r#"{"name":"Good","version":"1","permissions":[],"content_scripts":[]}"#;
        make_ext_dir(&tmp, "good", good, &[]);
        // Missing name → invalid
        let bad = r#"{"version":"1"}"#;
        make_ext_dir(&tmp, "bad", bad, &[]);

        let reg = ExtensionRegistry::load_from_dir(&tmp);
        assert_eq!(reg.len(), 1);
    }
}
