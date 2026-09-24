//! PERF-15: «память ресурсов сайта» — the subresources a page really used on
//! its previous visit, requested again together with the document on the next
//! one instead of waiting for the parser to discover them.
//!
//! Chrome remembers only hosts (for preconnect); a browser that knows the URLs
//! can start every first-wave request at once. On the `.tmp/seqlab` stand
//! (700 ms per response) that is the difference between ~0.7 s and ~2.8 s.
//!
//! # Life cycle of one visit
//!
//! 1. [`begin_visit`] (UI thread, `Lumen::start_streaming_load`, right after
//!    `PREFETCH_CACHE.reset`) closes the previous recording and opens one for
//!    the new page. It returns what the page used last time; the caller
//!    reserves a `PREFETCH_CACHE` slot for each and fetches it in the
//!    background ([`replay`]), so every consumer — `<script src>`, the
//!    stylesheet cascade, `<img>`, CSS backgrounds, `@font-face` — waits on
//!    that one request instead of sending its own.
//! 2. [`SiteMemorySink`] records every engine subresource load
//!    ([`Event::ResourceTimed`]) that starts within [`RECORD_WINDOW`] of the
//!    navigation — the first wave, not what the user clicks into later.
//! 3. After [`RECORD_WINDOW`] (timer thread) or at the next navigation /
//!    exit, the recording is committed ([`next_page`]).
//!
//! # Which URLs are replayed
//!
//! Many sites put a fresh token into their URLs on every visit — build
//! hashes, session ids, a random choice of images (google, paypal and naver
//! in the 2026-09-24 measurement: 22–82 URLs of a visit never came back).
//! Replaying the previous visit wholesale would waste a request on each of
//! them on *every* visit. So a page keeps two lists: `last` (what the
//! previous visit loaded) and `stable` (what the last two visits both
//! loaded). The first repeat visit replays `last` — the only list there is;
//! every later one replays only `stable`. A rotating URL is thus requested
//! in vain once per page, not once per visit. A replayed URL no consumer
//! read from its slot ([`crate::prefetch::PrefetchCache::was_used`]) counts
//! as not loaded by the page.
//!
//! # What is and is not remembered
//!
//! Only `script`/`style`/`image`/`font` destinations: these are the ones whose
//! consumer reads `PREFETCH_CACHE`. The page's own `fetch()`/XHR (mode and
//! credentials of the request are the page's to choose), iframes, media and
//! `<img crossorigin>` (BUG-1150, fetched in CORS mode past every cache) are
//! not recorded. The key is the document URL without fragment, not the whole
//! origin: another page of the same site would replay page-specific images.
//!
//! # Privacy
//!
//! Nothing leaves the machine — the list lives in `<data>/site_memory.json`
//! next to the HTTP cache, and requests go only to URLs the page itself
//! requested before. A `no_persistent_state`/Tor session keeps the list in
//! memory only; clearing the history clears it too (the page keys are visited
//! URLs). On/off rules — [`enabled`].
//!
//! # Known limits
//!
//! * The replay starts before the document response, so a cookie that
//!   response sets is not on the replayed requests yet.
//! * The `Referer` of a replayed request uses the default referrer policy,
//!   not the page's `<meta name=referrer>` (not parsed yet at that point).
//! * Tabs share one recorder (as they share the Resource Timing queue): a
//!   background tab loading during the window adds its resources too.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use lumen_core::event::Event;
use lumen_core::ext::EventSink;
use lumen_network::RequestDestination;

use crate::resource_base::ResourceBase;

/// How long after the navigation start a load still counts as the page's
/// first wave. Long enough for script-inserted images on a slow link, short
/// enough to leave out what the user opens by interacting.
pub(crate) const RECORD_WINDOW: Duration = Duration::from_secs(10);

/// At most this many resources replayed per page, earliest-started first.
/// Without HTTP/2 multiplexing (PERF-13) every parallel request to an origin
/// opens its own connection, so the wave is kept to what a page needs first.
const MAX_RESOURCES_PER_PAGE: usize = 32;

/// Pages kept; the least recently visited go first.
const MAX_PAGES: usize = 500;

/// A page not visited for this long is forgotten.
const MAX_AGE_SECS: u64 = 30 * 24 * 3600;

/// How many loads of a visit are kept as `last` for the next comparison —
/// more than [`MAX_RESOURCES_PER_PAGE`], so a URL that starts a little later
/// on the next visit still finds its match.
const MAX_LAST_PER_PAGE: usize = 64;

/// On-disk format version; a file with another one is ignored.
const FORMAT_VERSION: u32 = 2;

/// One remembered subresource.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Resource {
    pub(crate) url: String,
    /// Fetch destination, [`RequestDestination::as_fetch_dest`] spelling.
    pub(crate) dest: String,
}

impl Resource {
    /// The destination the replay must request with — the one the real
    /// consumer uses, so mixed-content blocking and `Accept` match.
    pub(crate) fn destination(&self) -> Option<RequestDestination> {
        remembered_destination(&self.dest)
    }
}

#[derive(Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Page {
    /// Last visit, unix seconds.
    visited: u64,
    /// What the previous visit loaded, in start order.
    last: Vec<Resource>,
    /// What the last two visits both loaded; `None` after a single visit.
    stable: Option<Vec<Resource>>,
}

impl Page {
    /// What the next visit replays — see the module docs. At most
    /// [`MAX_RESOURCES_PER_PAGE`], earliest-started first.
    fn replay_list(&self) -> &[Resource] {
        let list = self.stable.as_deref().unwrap_or(&self.last);
        &list[..list.len().min(MAX_RESOURCES_PER_PAGE)]
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct StoreFile {
    version: u32,
    pages: HashMap<String, Page>,
}

/// A visit being recorded.
struct Recording {
    generation: u64,
    page_key: String,
    /// Navigation start, unix-epoch ms (the clock of `ResourceTimed::start_ms`).
    started_ms: f64,
    /// `(start_ms, resource)` in arrival order, one per URL.
    seen: Vec<(f64, Resource)>,
    /// URLs this visit replayed from the previous one.
    replayed: Vec<String>,
}

struct State {
    /// `None` until first use, then the loaded (or empty) store.
    store: Option<StoreFile>,
    recording: Option<Recording>,
}

static STATE: Mutex<State> = Mutex::new(State { store: None, recording: None });

/// Where the list is kept; `None` = memory only (private session).
/// `LUMEN_SITE_MEMORY_FILE=<path>` names the file explicitly and wins even in
/// an automation session: a harness measuring repeat visits across fresh
/// processes (`perf_audit.py --mode compat`) needs the list to survive them.
fn store_path() -> Option<PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        if let Some(path) = std::env::var_os("LUMEN_SITE_MEMORY_FILE").filter(|p| !p.is_empty()) {
            return Some(PathBuf::from(path));
        }
        let cfg = crate::config::global();
        let private =
            cfg.no_persistent_state || cfg.http_profile == lumen_network::HttpProfile::TorBrowser;
        (!private).then(|| crate::adblock::browser_data_dir().join("site_memory.json"))
    })
    .clone()
}

/// Whether this process remembers and replays. On by default in a user's
/// window; off in automation (`--bidi-port`/`--mcp*`) and `--deterministic`
/// runs, where one process opens test after test and a replay from the
/// previous one would change what the server sees (WPT counts requests).
/// `LUMEN_SITE_MEMORY=1|0` overrides either way — a perf harness driving the
/// window over MCP turns it on to measure repeat visits.
pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| match std::env::var("LUMEN_SITE_MEMORY").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => !std::env::args().any(|a| {
            matches!(
                a.as_str(),
                "--bidi-port" | "--mcp-live-port" | "--mcp" | "--mcp-port" | "--deterministic"
            )
        }),
    })
}

fn remembered_destination(dest: &str) -> Option<RequestDestination> {
    Some(match dest {
        "script" => RequestDestination::Script,
        "style" => RequestDestination::Style,
        "image" => RequestDestination::Image,
        "font" => RequestDestination::Font,
        _ => return None,
    })
}

/// The key a page is remembered under: its URL without fragment, only for
/// `http`/`https` documents.
pub(crate) fn page_key(url: &str) -> Option<String> {
    let parsed = lumen_core::url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let href = parsed.href_whatwg();
    Some(href.split('#').next().unwrap_or(href).to_owned())
}

fn now_unix_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn now_unix_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

fn load_store(path: Option<&PathBuf>) -> StoreFile {
    let Some(path) = path else { return StoreFile::default() };
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoreFile>(&bytes).ok())
        .filter(|s| s.version == FORMAT_VERSION)
        .unwrap_or_default()
}

fn save_store(store: &StoreFile, path: &PathBuf) {
    let Ok(json) = serde_json::to_vec(store) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // temp + rename: a second Lumen process never reads a half-written file.
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Close the current recording, open one for `page_url` under navigation
/// `generation`, and return what the page used on its previous visit.
///
/// Must run after `PREFETCH_CACHE.reset(generation)`. A non-web URL or a
/// disabled feature closes the old recording and starts nothing.
pub(crate) fn begin_visit(page_url: &str, generation: u64) -> Vec<Resource> {
    let mut state = lock();
    commit_locked(&mut state, None);
    let Some(key) = page_key(page_url).filter(|_| enabled()) else { return Vec::new() };
    let path = store_path();
    let store = state.store.get_or_insert_with(|| load_store(path.as_ref()));
    let now = now_unix_s();
    let remembered: Vec<Resource> = store
        .pages
        .get(&key)
        .filter(|p| now.saturating_sub(p.visited) <= MAX_AGE_SECS)
        .map(|p| p.replay_list().iter().filter(|r| r.destination().is_some()).cloned().collect())
        .unwrap_or_default();
    state.recording = Some(Recording {
        generation,
        page_key: key,
        started_ms: now_unix_ms(),
        seen: Vec::new(),
        replayed: remembered.iter().map(|r| r.url.clone()).collect(),
    });
    drop(state);
    // Commit on its own after the window: a window closed with `taskkill`, or
    // left on one page, never reaches the next navigation or `exiting`.
    let _ = std::thread::Builder::new().name("lumen-site-memory".to_owned()).spawn(move || {
        std::thread::sleep(RECORD_WINDOW);
        let mut state = lock();
        commit_locked(&mut state, Some(generation));
    });
    remembered
}

/// Commit the current recording now (navigation away, exit).
pub(crate) fn finish_visit() {
    commit_locked(&mut lock(), None);
}

/// Forget every page (history cleared).
pub(crate) fn forget_all() {
    let mut state = lock();
    state.recording = None;
    state.store = Some(StoreFile { version: FORMAT_VERSION, pages: HashMap::new() });
    if let Some(path) = store_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Commit the open recording — only if it belongs to `only_generation` when
/// that is given (the timer of a visit that was already superseded is a no-op).
fn commit_locked(state: &mut State, only_generation: Option<u64>) {
    if only_generation.is_some_and(|g| state.recording.as_ref().is_none_or(|r| r.generation != g)) {
        return;
    }
    let Some(rec) = state.recording.take() else { return };
    let loaded = loaded_by_page(rec.seen, &rec.replayed, |url| {
        crate::prefetch::PREFETCH_CACHE.was_used(rec.generation, url)
    });
    let path = store_path();
    let store = state.store.get_or_insert_with(|| load_store(path.as_ref()));
    store.version = FORMAT_VERSION;
    let now = now_unix_s();
    match next_page(store.pages.get(&rec.page_key), loaded, now) {
        Some(page) => {
            store.pages.insert(rec.page_key, page);
        }
        None => {
            store.pages.remove(&rec.page_key);
        }
    }
    prune(store, now);
    if let Some(path) = path {
        save_store(store, &path);
    }
}

/// What the page itself loaded this visit, in start order: the recorded loads
/// minus the replayed URLs no consumer took (those were our request, not the
/// page's), capped at [`MAX_LAST_PER_PAGE`].
fn loaded_by_page(
    mut seen: Vec<(f64, Resource)>,
    replayed: &[String],
    was_used: impl Fn(&str) -> bool,
) -> Vec<Resource> {
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    seen.into_iter()
        .map(|(_, r)| r)
        .filter(|r| !replayed.contains(&r.url) || was_used(&r.url))
        .take(MAX_LAST_PER_PAGE)
        .collect()
}

/// The page entry after a visit that loaded `loaded`; `None` when there is
/// nothing to keep. `stable` is this visit's loads that the previous visit
/// had too, in this visit's order, capped at [`MAX_RESOURCES_PER_PAGE`].
fn next_page(prev: Option<&Page>, loaded: Vec<Resource>, now: u64) -> Option<Page> {
    if loaded.is_empty() {
        return None;
    }
    let stable = prev.map(|p| {
        loaded
            .iter()
            .filter(|r| p.last.iter().any(|q| q.url == r.url))
            .take(MAX_RESOURCES_PER_PAGE)
            .cloned()
            .collect()
    });
    let mut last = loaded;
    last.truncate(MAX_LAST_PER_PAGE);
    Some(Page { visited: now, last, stable })
}

fn prune(store: &mut StoreFile, now: u64) {
    store.pages.retain(|_, p| now.saturating_sub(p.visited) <= MAX_AGE_SECS);
    if store.pages.len() > MAX_PAGES {
        let mut by_age: Vec<(u64, String)> =
            store.pages.iter().map(|(k, p)| (p.visited, k.clone())).collect();
        by_age.sort();
        for (_, key) in by_age.into_iter().take(store.pages.len() - MAX_PAGES) {
            store.pages.remove(&key);
        }
    }
}

/// Note one completed engine load for the open recording.
fn observe(url: &str, destination: &str, start_ms: f64) {
    if remembered_destination(destination).is_none() {
        return;
    }
    let mut state = lock();
    let Some(rec) = state.recording.as_mut() else { return };
    if start_ms < rec.started_ms || start_ms - rec.started_ms > RECORD_WINDOW.as_secs_f64() * 1000.0 {
        return;
    }
    if rec.seen.iter().any(|(_, r)| r.url == url) {
        return;
    }
    rec.seen.push((start_ms, Resource { url: url.to_owned(), dest: destination.to_owned() }));
}

/// Start the remembered requests for navigation `generation`, each into its
/// own `PREFETCH_CACHE` slot, reserved here on the caller's thread so no
/// consumer can slip past it to the network.
pub(crate) fn replay(
    resources: Vec<Resource>,
    generation: u64,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: &Arc<lumen_storage::CookieJar>,
) {
    for resource in resources {
        let Some(destination) = resource.destination() else { continue };
        let Ok(parsed) = lumen_core::url::Url::parse(&resource.url) else { continue };
        let Some(reservation) = crate::prefetch::PREFETCH_CACHE.reserve(generation, &resource.url)
        else {
            continue;
        };
        let base = base.clone();
        let sink = Arc::clone(sink);
        let cookie_jar = Arc::clone(cookie_jar);
        // A failed spawn drops `reservation`, which publishes an error: the
        // consumer then fetches the resource itself.
        let _ = std::thread::Builder::new().name("lumen-site-replay".to_owned()).spawn(move || {
            let client = base.http_client_for_subresource(sink, Some(cookie_jar));
            reservation.fill(
                client
                    .fetch_subresource_with_content_type(&parsed, destination)
                    .map(|(body, content_type)| crate::prefetch::CachedResource { body, content_type })
                    .map_err(|e| e.to_string()),
            );
        });
    }
}

/// Sink tap feeding [`observe`]; forwards every event to `inner` unchanged.
pub struct SiteMemorySink {
    /// The next sink in the chain.
    pub inner: Arc<dyn EventSink>,
}

impl EventSink for SiteMemorySink {
    fn emit(&self, event: &Event) {
        if let Event::ResourceTimed { url, destination, start_ms, .. } = event {
            observe(url, destination, *start_ms);
        }
        self.inner.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(url: &str, dest: &str) -> Resource {
        Resource { url: url.to_owned(), dest: dest.to_owned() }
    }

    #[test]
    fn page_key_drops_fragment_and_non_web_schemes() {
        assert_eq!(page_key("https://a.test/p?q=1#top").as_deref(), Some("https://a.test/p?q=1"));
        assert_eq!(page_key("http://127.0.0.1:18200/").as_deref(), Some("http://127.0.0.1:18200/"));
        assert_eq!(page_key("file:///C:/x.html"), None);
        assert_eq!(page_key("about:blank"), None);
    }

    #[test]
    fn only_consumer_read_destinations_are_remembered() {
        for dest in ["script", "style", "image", "font"] {
            assert!(remembered_destination(dest).is_some(), "{dest}");
        }
        for dest in ["", "document", "video", "prefetch", "worker"] {
            assert!(remembered_destination(dest).is_none(), "{dest}");
        }
    }

    #[test]
    fn loaded_by_page_orders_caps_and_drops_unused_replays() {
        let seen = vec![
            (30.0, res("https://a.test/late.png", "image")),
            (10.0, res("https://a.test/app.js", "script")),
            (20.0, res("https://a.test/gone.css", "style")),
        ];
        let replayed = vec!["https://a.test/gone.css".to_owned(), "https://a.test/app.js".to_owned()];
        let kept = loaded_by_page(seen, &replayed, |url| url.ends_with("app.js"));
        assert_eq!(
            kept,
            vec![res("https://a.test/app.js", "script"), res("https://a.test/late.png", "image")]
        );

        let many: Vec<_> = (0..MAX_LAST_PER_PAGE + 5)
            .map(|i| (i as f64, res(&format!("https://a.test/{i}.png"), "image")))
            .collect();
        let kept = loaded_by_page(many, &[], |_| false);
        assert_eq!(kept.len(), MAX_LAST_PER_PAGE);
        assert_eq!(kept[0].url, "https://a.test/0.png");
    }

    #[test]
    fn rotating_urls_are_replayed_once_then_left_out() {
        let fixed = res("https://a.test/app.js", "script");
        let visit = |token: &str| vec![fixed.clone(), res(&format!("https://a.test/{token}.png"), "image")];

        // First visit: nothing to compare with, the next one replays all.
        let p1 = next_page(None, visit("t1"), 1).unwrap();
        assert_eq!(p1.stable, None);
        assert_eq!(p1.replay_list(), visit("t1").as_slice());

        // Second visit loads another token: only the fixed URL is stable,
        // and from now on only it is replayed.
        let p2 = next_page(Some(&p1), visit("t2"), 2).unwrap();
        assert_eq!(p2.replay_list(), std::slice::from_ref(&fixed));
        let p3 = next_page(Some(&p2), visit("t3"), 3).unwrap();
        assert_eq!(p3.replay_list(), std::slice::from_ref(&fixed));
        assert_eq!(p3.last, visit("t3"), "`last` follows the latest visit");

        // A visit that loaded nothing drops the page.
        assert_eq!(next_page(Some(&p3), Vec::new(), 4), None);
    }

    #[test]
    fn replay_list_is_capped() {
        let many: Vec<_> =
            (0..MAX_LAST_PER_PAGE).map(|i| res(&format!("https://a.test/{i}.png"), "image")).collect();
        let first = next_page(None, many.clone(), 1).unwrap();
        assert_eq!(first.replay_list().len(), MAX_RESOURCES_PER_PAGE);
        let second = next_page(Some(&first), many, 2).unwrap();
        assert_eq!(second.replay_list().len(), MAX_RESOURCES_PER_PAGE);
    }

    #[test]
    fn prune_drops_stale_and_oldest_over_cap() {
        let mut store = StoreFile { version: FORMAT_VERSION, pages: HashMap::new() };
        let now = 10 * MAX_AGE_SECS;
        let page = |visited| Page { visited, last: vec![], stable: None };
        store.pages.insert("stale".into(), page(now - MAX_AGE_SECS - 1));
        for i in 0..MAX_PAGES + 2 {
            store.pages.insert(format!("p{i}"), page(now - 1000 + i as u64));
        }
        prune(&mut store, now);
        assert_eq!(store.pages.len(), MAX_PAGES);
        assert!(!store.pages.contains_key("stale"));
        assert!(!store.pages.contains_key("p0") && !store.pages.contains_key("p1"));
        assert!(store.pages.contains_key(&format!("p{}", MAX_PAGES + 1)));
    }

    #[test]
    fn store_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("lumen-site-memory-{}", std::process::id()));
        let path = dir.join("site_memory.json");
        let mut store = StoreFile { version: FORMAT_VERSION, pages: HashMap::new() };
        store.pages.insert(
            "https://a.test/".into(),
            Page { visited: 7, last: vec![res("https://a.test/a.css", "style")], stable: Some(vec![]) },
        );
        save_store(&store, &path);
        let loaded = load_store(Some(&path));
        assert_eq!(loaded.pages["https://a.test/"], store.pages["https://a.test/"]);
        // A file of another format version is ignored, not misread.
        std::fs::write(&path, br#"{"version":99,"pages":{}}"#).unwrap();
        assert!(load_store(Some(&path)).pages.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
