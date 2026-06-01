//! Omnibox alias resolution (§7B.4).
//!
//! **Bang aliases** — `!g <q>` / `!gh <q>` / any custom `!<trigger>` — expand
//! to a URL via an [`OmniboxAlias`] template from the storage layer.
//!
//! **@ actions** — built-in keywords for special commands:
//! - `@notes <text>` → [`AliasAction::CreateNote`]
//! - `@read-later <url>` → [`AliasAction::SaveReadLater`]
//!
//! Call [`resolve`] at omnibox commit time (Enter) to intercept a raw input
//! string and return the appropriate action.  `None` means no alias matched —
//! treat the string as a plain URL / search query.

use lumen_storage::OmniboxAlias;

// ── AliasAction ───────────────────────────────────────────────────────────────

/// Action produced by resolving a raw omnibox input against the alias table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasAction {
    /// Navigate to this fully-expanded URL (bang alias resolved).
    Navigate(String),
    /// Save a quick note with this text.  Shell stores it in the notes list.
    CreateNote(String),
    /// Add this URL to the read-later list.
    SaveReadLater(String),
}

// ── Resolver ──────────────────────────────────────────────────────────────────

/// Resolve `input` against the alias table and built-in `@` actions.
///
/// Matching rules (in order):
/// 1. Input starts with `!` → parse as `!<trigger> <query>`, look up trigger
///    in `aliases`.  If found, expand and return `Navigate`.
/// 2. Input starts with `@notes ` → return `CreateNote`.
/// 3. Input starts with `@read-later ` → return `SaveReadLater`.
/// 4. Otherwise → `None` (caller handles as plain URL / search query).
pub fn resolve(input: &str, aliases: &[OmniboxAlias]) -> Option<AliasAction> {
    let input = input.trim();

    if let Some(bang_rest) = input.strip_prefix('!') {
        return resolve_bang(bang_rest, aliases);
    }

    if let Some(rest) = input.strip_prefix("@notes") {
        let text = rest.trim_start();
        if !text.is_empty() {
            return Some(AliasAction::CreateNote(text.to_owned()));
        }
    }

    if let Some(rest) = input.strip_prefix("@read-later") {
        let url = rest.trim_start();
        if !url.is_empty() {
            return Some(AliasAction::SaveReadLater(url.to_owned()));
        }
    }

    None
}

/// Resolve a bang fragment (input after the leading `!`).
fn resolve_bang(bang_rest: &str, aliases: &[OmniboxAlias]) -> Option<AliasAction> {
    // Split into trigger-word and optional query.
    // `!g rust` → trigger = "!g", query = "rust"
    // `!g`      → trigger = "!g", query = ""  (navigate to bare template)
    let (trigger_word, query) = match bang_rest.find(char::is_whitespace) {
        Some(pos) => (&bang_rest[..pos], bang_rest[pos..].trim_start()),
        None => (bang_rest, ""),
    };
    let full_trigger = format!("!{trigger_word}");

    let alias = aliases.iter().find(|a| a.trigger == full_trigger)?;
    let expanded = expand_template(&alias.expansion, query);
    Some(AliasAction::Navigate(expanded))
}

/// Replace `{query}` in an alias template with the URL-encoded query string.
fn expand_template(template: &str, query: &str) -> String {
    template.replace("{query}", &url_encode(query))
}

/// URL-encode a query string: percent-encode everything except RFC 3986 unreserved chars.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    for byte in s.as_bytes() {
        let c = *byte;
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            out.push('%');
            out.push(HEX[(c >> 4) as usize] as char);
            out.push(HEX[(c & 0x0F) as usize] as char);
        }
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn google() -> OmniboxAlias {
        OmniboxAlias {
            trigger: "!g".to_owned(),
            expansion: "https://www.google.com/search?q={query}".to_owned(),
        }
    }

    fn github() -> OmniboxAlias {
        OmniboxAlias {
            trigger: "!gh".to_owned(),
            expansion: "https://github.com/search?q={query}&type=repositories".to_owned(),
        }
    }

    fn aliases() -> Vec<OmniboxAlias> {
        vec![google(), github()]
    }

    // ── bang resolution ───────────────────────────────────────────────────────

    #[test]
    fn bang_google_basic() {
        let action = resolve("!g rust lang", &aliases());
        assert_eq!(
            action,
            Some(AliasAction::Navigate(
                "https://www.google.com/search?q=rust%20lang".to_owned()
            ))
        );
    }

    #[test]
    fn bang_github_basic() {
        let action = resolve("!gh tokio", &aliases());
        assert_eq!(
            action,
            Some(AliasAction::Navigate(
                "https://github.com/search?q=tokio&type=repositories".to_owned()
            ))
        );
    }

    #[test]
    fn bang_with_special_chars() {
        let action = resolve("!g a&b=c", &aliases());
        let url = match action {
            Some(AliasAction::Navigate(u)) => u,
            _ => panic!("expected Navigate"),
        };
        assert!(url.contains("a%26b%3Dc"), "& and = must be percent-encoded");
    }

    #[test]
    fn bang_no_query_expands_to_template() {
        let action = resolve("!g", &aliases());
        // query is empty → {query} replaced with ""
        assert_eq!(
            action,
            Some(AliasAction::Navigate(
                "https://www.google.com/search?q=".to_owned()
            ))
        );
    }

    #[test]
    fn bang_unknown_trigger_returns_none() {
        assert_eq!(resolve("!yt rust", &aliases()), None);
    }

    #[test]
    fn bang_custom_alias() {
        let mut al = aliases();
        al.push(OmniboxAlias {
            trigger: "!yt".to_owned(),
            expansion: "https://www.youtube.com/results?search_query={query}".to_owned(),
        });
        let action = resolve("!yt cats", &al);
        assert_eq!(
            action,
            Some(AliasAction::Navigate(
                "https://www.youtube.com/results?search_query=cats".to_owned()
            ))
        );
    }

    #[test]
    fn bang_leading_whitespace_ignored() {
        let action = resolve("  !g rust  ", &aliases());
        assert!(matches!(action, Some(AliasAction::Navigate(_))));
    }

    // ── @notes ────────────────────────────────────────────────────────────────

    #[test]
    fn notes_basic() {
        let action = resolve("@notes remember to call Bob", &[]);
        assert_eq!(
            action,
            Some(AliasAction::CreateNote("remember to call Bob".to_owned()))
        );
    }

    #[test]
    fn notes_empty_returns_none() {
        assert_eq!(resolve("@notes", &[]), None);
        assert_eq!(resolve("@notes   ", &[]), None);
    }

    #[test]
    fn notes_with_leading_whitespace() {
        let action = resolve("  @notes hello", &[]);
        assert_eq!(action, Some(AliasAction::CreateNote("hello".to_owned())));
    }

    // ── @read-later ───────────────────────────────────────────────────────────

    #[test]
    fn read_later_basic() {
        let action = resolve("@read-later https://example.com/article", &[]);
        assert_eq!(
            action,
            Some(AliasAction::SaveReadLater(
                "https://example.com/article".to_owned()
            ))
        );
    }

    #[test]
    fn read_later_empty_returns_none() {
        assert_eq!(resolve("@read-later", &[]), None);
        assert_eq!(resolve("@read-later   ", &[]), None);
    }

    // ── plain input ───────────────────────────────────────────────────────────

    #[test]
    fn plain_url_returns_none() {
        assert_eq!(resolve("https://rust-lang.org", &aliases()), None);
    }

    #[test]
    fn plain_query_returns_none() {
        assert_eq!(resolve("rust programming", &aliases()), None);
    }

    #[test]
    fn empty_returns_none() {
        assert_eq!(resolve("", &aliases()), None);
    }

    // ── url_encode ────────────────────────────────────────────────────────────

    #[test]
    fn encode_space_as_percent20() {
        let enc = super::url_encode("hello world");
        assert_eq!(enc, "hello%20world");
    }

    #[test]
    fn encode_unreserved_unchanged() {
        let enc = super::url_encode("abc-_.~");
        assert_eq!(enc, "abc-_.~");
    }

    #[test]
    fn encode_cyrillic() {
        let enc = super::url_encode("привет");
        assert!(enc.starts_with('%'), "should be percent-encoded");
        assert!(!enc.contains(' '));
    }
}
