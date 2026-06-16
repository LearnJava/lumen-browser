//! Ad-block external filter-list orchestration (storage + network + parsing).
//!
//! Replaces the binary-embedded `DefaultFilterList` with downloadable external
//! lists (EasyList, EasyPrivacy), cached on disk and refreshed periodically —
//! like a Chrome ad-blocker extension. The matching engine
//! (`lumen_network::EasyListFilter`) is reused unchanged; this module adds the
//! load / store / refresh layer on top.
//!
//! **Storage layout (portable, browser-folder only — never OS dirs):**
//!
//! ```text
//! <exe_dir>/data/adblock/
//!   ├── adblock.db        SQLite (AdblockStore): subscriptions + list_meta
//!   ├── custom-rules.txt  user rules (Phase 3)
//!   └── lists/<slug>.txt   downloaded list bodies (read into RAM at startup)
//! ```
//!
//! **Hot path stays in RAM:** `should_block(url)` runs against the in-memory
//! `EasyListFilter`; the DB/files are touched only at startup and on refresh.
//! See `docs/tasks/p2-adblock-filter-lists.md`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::ext::FilterListSource as _;
use lumen_core::url::Url;
use lumen_network::{
    ConditionalFetch, DefaultFilterList, EasyListFilter, HttpClient,
    install_global_adblock_filter,
};
use lumen_storage::adblock::{AdblockStore, ListMeta, Subscription};

/// EasyList recommends an expiry of ~4 days; refresh no more often than that.
pub const REFRESH_INTERVAL_SECS: i64 = 4 * 24 * 3600;

// ── Filesystem layout ────────────────────────────────────────────────────────

/// Root of all browser user data (portable): `<exe_dir>/data`.
///
/// Subsystems place their data in named subfolders (`adblock/`, …). Falls back
/// to `./data` relative to the current directory if the executable path is
/// unavailable; never escapes into OS config/cache directories.
pub fn browser_data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("data")))
        .unwrap_or_else(|| PathBuf::from("data"))
}

/// `<data>/adblock` — root of the ad-block subsystem's files.
pub fn adblock_dir() -> PathBuf {
    browser_data_dir().join("adblock")
}

/// `<data>/adblock/lists` — downloaded list bodies.
pub fn lists_dir() -> PathBuf {
    adblock_dir().join("lists")
}

/// Path to the SQLite store (`adblock.db`).
pub fn db_path() -> PathBuf {
    adblock_dir().join("adblock.db")
}

/// Create `data/adblock/lists/` if missing (best-effort).
pub fn ensure_dirs() {
    let _ = std::fs::create_dir_all(lists_dir());
}

// ── Default subscriptions ──────────────────────────────────────────────────

/// The lists seeded on first run: EasyList (ads) + EasyPrivacy (trackers).
pub fn default_subscriptions() -> Vec<Subscription> {
    vec![
        Subscription {
            url: "https://easylist.to/easylist/easylist.txt".to_owned(),
            title: "EasyList".to_owned(),
            enabled: true,
        },
        Subscription {
            url: "https://easylist.to/easylist/easyprivacy.txt".to_owned(),
            title: "EasyPrivacy".to_owned(),
            enabled: true,
        },
    ]
}

// ── Slug + hashing helpers (pure) ──────────────────────────────────────────

/// Sanitize a title into a filesystem-safe slug: lowercase, `[a-z0-9-]` only,
/// runs of other characters collapse to a single `-`, trimmed at the ends.
fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = false;
    for ch in title.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let s = out.trim_matches('-').to_owned();
    if s.is_empty() { "list".to_owned() } else { s }
}

/// Assign a unique slug to each subscription, resolving collisions with a
/// numeric suffix (`-2`, `-3`, …). Deterministic for a given input order.
fn assign_slugs(subs: &[Subscription]) -> Vec<(Subscription, String)> {
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(subs.len());
    for sub in subs {
        let base = slugify(&sub.title);
        let count = seen.entry(base.clone()).or_insert(0);
        *count += 1;
        let slug = if *count == 1 {
            base
        } else {
            format!("{base}-{count}")
        };
        out.push((sub.clone(), slug));
    }
    out
}

/// Stable 64-bit FNV-1a hash of the body text, as lowercase hex.
///
/// Deterministic across runs and Rust versions (unlike `DefaultHasher`), so it
/// can be persisted in `list_meta.content_hash` and compared on the next fetch
/// to skip re-parsing an unchanged `200 OK` body.
fn content_hash(text: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether a list is due for a refresh: never fetched, or older than the
/// refresh interval.
fn is_expired(meta: Option<&ListMeta>, now: i64) -> bool {
    match meta {
        None => true,
        Some(m) => now - m.fetched_at >= REFRESH_INTERVAL_SECS,
    }
}

// ── Load + install (offline-first) ─────────────────────────────────────────

/// Read the enabled subscriptions' cached bodies from disk, merge them into a
/// single text, parse once, and install the result as the global filter.
///
/// Merging into **one** string is required so `@@` exceptions from one list
/// can cancel block rules from another (the matcher resolves exceptions
/// globally). If no cached bodies exist yet (first run before refresh), falls
/// back to the binary-embedded [`DefaultFilterList`] so blocking works
/// immediately. Returns the number of rules installed.
pub fn load_and_install(store: &AdblockStore) -> usize {
    let subs = store.list_subscriptions().unwrap_or_default();
    let enabled: Vec<Subscription> = subs.into_iter().filter(|s| s.enabled).collect();

    let mut merged = String::new();
    for (_, slug) in assign_slugs(&enabled) {
        let path = lists_dir().join(format!("{slug}.txt"));
        if let Ok(body) = std::fs::read_to_string(&path) {
            merged.push_str(&body);
            merged.push('\n');
        }
    }

    let filter = if merged.trim().is_empty() {
        // No external lists cached yet — keep blocking via the bundled list.
        let rules = DefaultFilterList.fetch_rules().unwrap_or_default();
        EasyListFilter::parse(&rules)
    } else {
        EasyListFilter::parse(&merged)
    };
    let count = filter.rule_count();
    install_global_adblock_filter(Arc::new(filter));
    count
}

// ── Refresh (conditional GET) ──────────────────────────────────────────────

/// Conditionally refresh all enabled subscriptions over the network.
///
/// For each list older than [`REFRESH_INTERVAL_SECS`], issues a conditional GET
/// (`If-None-Match` / `If-Modified-Since` from `list_meta`). On `304` only the
/// fetch timestamp is bumped; on `200` the body is written to `lists/<slug>.txt`
/// and metadata is updated. Network errors are non-fatal — the cached copy is
/// kept and the error logged.
///
/// Returns `true` if any list's content actually changed, signalling the caller
/// to re-run [`load_and_install`] to hot-swap the reparsed filter.
pub fn refresh(store: &AdblockStore, client: &HttpClient) -> bool {
    let subs = store.list_subscriptions().unwrap_or_default();
    let enabled: Vec<Subscription> = subs.into_iter().filter(|s| s.enabled).collect();
    let now = now_unix();
    let mut any_changed = false;

    for (sub, slug) in assign_slugs(&enabled) {
        let meta = store.get_meta(&slug).unwrap_or(None);
        if !is_expired(meta.as_ref(), now) {
            continue;
        }
        let Ok(url) = Url::parse(&sub.url) else {
            eprintln!("adblock: invalid subscription URL: {}", sub.url);
            continue;
        };
        let (etag, last_modified) = meta
            .as_ref()
            .map(|m| (m.etag.clone(), m.last_modified.clone()))
            .unwrap_or((None, None));

        match client.fetch_conditional(&url, etag.as_deref(), last_modified.as_deref()) {
            Ok(result) => {
                match apply_fetch_result(store, &slug, &sub.url, meta.as_ref(), result, now) {
                    Ok(true) => any_changed = true,
                    Ok(false) => {}
                    Err(e) => eprintln!("adblock: store error for {slug}: {e}"),
                }
            }
            Err(e) => eprintln!("adblock: refresh failed for {}: {e}", sub.url),
        }
    }
    any_changed
}

/// Apply one conditional-GET outcome: persist the body/meta and report whether
/// the parsed content changed (so the caller knows to reinstall the filter).
///
/// Pure with respect to the network — exercised directly in tests with
/// synthetic [`ConditionalFetch`] values.
fn apply_fetch_result(
    store: &AdblockStore,
    slug: &str,
    url: &str,
    prev: Option<&ListMeta>,
    result: ConditionalFetch,
    now: i64,
) -> lumen_core::Result<bool> {
    match result {
        ConditionalFetch::NotModified => {
            // Bump fetched_at so we don't re-poll until the next interval.
            let mut meta = prev.cloned().unwrap_or(ListMeta {
                slug: slug.to_owned(),
                url: url.to_owned(),
                etag: None,
                last_modified: None,
                fetched_at: now,
                rule_count: 0,
                content_hash: None,
            });
            meta.fetched_at = now;
            store.upsert_meta(&meta)?;
            Ok(false)
        }
        ConditionalFetch::Modified {
            body,
            etag,
            last_modified,
        } => {
            let text = String::from_utf8_lossy(&body).into_owned();
            let hash = content_hash(&text);
            let unchanged = prev.and_then(|m| m.content_hash.clone()).as_deref() == Some(hash.as_str());

            // Always (re)write the body file so the on-disk cache matches meta.
            let path = lists_dir().join(format!("{slug}.txt"));
            if let Err(e) = std::fs::write(&path, &text) {
                eprintln!("adblock: cannot write {}: {e}", path.display());
            }

            let rule_count = if unchanged {
                prev.map_or(0, |m| m.rule_count)
            } else {
                EasyListFilter::parse(&text).rule_count() as i64
            };

            store.upsert_meta(&ListMeta {
                slug: slug.to_owned(),
                url: url.to_owned(),
                etag,
                last_modified,
                fetched_at: now,
                rule_count,
                content_hash: Some(hash),
            })?;
            Ok(!unchanged)
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("EasyList"), "easylist");
        assert_eq!(slugify("EasyPrivacy"), "easyprivacy");
        assert_eq!(slugify("My Custom List!"), "my-custom-list");
        assert_eq!(slugify("  spaced  "), "spaced");
        assert_eq!(slugify("***"), "list");
    }

    #[test]
    fn assign_slugs_resolves_collisions() {
        let subs = vec![
            Subscription { url: "a".into(), title: "Ads".into(), enabled: true },
            Subscription { url: "b".into(), title: "Ads".into(), enabled: true },
            Subscription { url: "c".into(), title: "Ads".into(), enabled: true },
        ];
        let slugs: Vec<String> = assign_slugs(&subs).into_iter().map(|(_, s)| s).collect();
        assert_eq!(slugs, vec!["ads", "ads-2", "ads-3"]);
    }

    #[test]
    fn content_hash_is_stable_and_distinct() {
        assert_eq!(content_hash("||tracker.com^"), content_hash("||tracker.com^"));
        assert_ne!(content_hash("||a.com^"), content_hash("||b.com^"));
        // Known FNV-1a vector for empty string.
        assert_eq!(content_hash(""), "cbf29ce484222325");
    }

    #[test]
    fn is_expired_logic() {
        assert!(is_expired(None, 1000));
        let fresh = ListMeta {
            slug: "x".into(), url: "u".into(), etag: None, last_modified: None,
            fetched_at: 1000, rule_count: 0, content_hash: None,
        };
        assert!(!is_expired(Some(&fresh), 1000 + REFRESH_INTERVAL_SECS - 1));
        assert!(is_expired(Some(&fresh), 1000 + REFRESH_INTERVAL_SECS));
    }

    #[test]
    fn not_modified_bumps_timestamp_no_reparse() {
        let store = AdblockStore::open_in_memory().unwrap();
        let prev = ListMeta {
            slug: "easylist".into(),
            url: "https://easylist.to/easylist.txt".into(),
            etag: Some("\"v1\"".into()),
            last_modified: None,
            fetched_at: 100,
            rule_count: 5,
            content_hash: Some("abc".into()),
        };
        store.upsert_meta(&prev).unwrap();
        let changed = apply_fetch_result(
            &store, "easylist", &prev.url, Some(&prev),
            ConditionalFetch::NotModified, 999,
        )
        .unwrap();
        assert!(!changed, "304 must not trigger reinstall");
        let got = store.get_meta("easylist").unwrap().unwrap();
        assert_eq!(got.fetched_at, 999);
        assert_eq!(got.rule_count, 5, "rule_count preserved on 304");
        assert_eq!(got.etag.as_deref(), Some("\"v1\""));
    }

    #[test]
    fn modified_same_hash_no_reinstall() {
        let store = AdblockStore::open_in_memory().unwrap();
        ensure_dirs();
        let body = b"||tracker.com^\n".to_vec();
        let hash = content_hash(&String::from_utf8_lossy(&body));
        let prev = ListMeta {
            slug: "modtest-same".into(),
            url: "https://x/list.txt".into(),
            etag: None, last_modified: None,
            fetched_at: 1, rule_count: 1,
            content_hash: Some(hash),
        };
        store.upsert_meta(&prev).unwrap();
        let changed = apply_fetch_result(
            &store, "modtest-same", &prev.url, Some(&prev),
            ConditionalFetch::Modified { body, etag: Some("\"new\"".into()), last_modified: None },
            42,
        )
        .unwrap();
        assert!(!changed, "identical body hash must not trigger reinstall");
        let got = store.get_meta("modtest-same").unwrap().unwrap();
        assert_eq!(got.fetched_at, 42);
        assert_eq!(got.etag.as_deref(), Some("\"new\""));
    }

    #[test]
    fn modified_changed_triggers_reinstall_and_writes_file() {
        let store = AdblockStore::open_in_memory().unwrap();
        ensure_dirs();
        let prev = ListMeta {
            slug: "modtest-changed".into(),
            url: "https://x/list.txt".into(),
            etag: None, last_modified: None,
            fetched_at: 1, rule_count: 1,
            content_hash: Some("oldhash".into()),
        };
        store.upsert_meta(&prev).unwrap();
        let body = b"||a.com^\n||b.com^\n||c.com^\n".to_vec();
        let changed = apply_fetch_result(
            &store, "modtest-changed", &prev.url, Some(&prev),
            ConditionalFetch::Modified { body, etag: None, last_modified: Some("Mon".into()) },
            7,
        )
        .unwrap();
        assert!(changed, "new content must trigger reinstall");
        let got = store.get_meta("modtest-changed").unwrap().unwrap();
        assert_eq!(got.rule_count, 3, "reparsed rule count");
        // Body written to disk.
        let path = lists_dir().join("modtest-changed.txt");
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("||b.com^"));
        let _ = std::fs::remove_file(&path);
    }
}
