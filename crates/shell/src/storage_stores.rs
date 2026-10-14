//! Per-origin storage partitions handed to a document: localStorage,
//! sessionStorage, IndexedDB and the Service Worker registration store.
//!
//! All four are keyed by the origin derived from a [`ResourceBase`]; a `file:`
//! base has no persistent partition, so each getter answers `None` there and
//! the caller falls back to an ephemeral in-memory store.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;
/// Returns the portable directory for per-origin IndexedDB SQLite files,
/// creating it if it does not exist.
///
/// Path: `<exe_dir>/data/idb/` via [`adblock::browser_data_dir`] — the project's
/// portable-data convention (user decision 2026-06-16: keep all browser data in
/// the browser folder, never in OS dirs like `%APPDATA%`/`~/.config`).
///
/// Returns `None` when directory creation fails — the caller falls back to
/// ephemeral in-memory IDB storage for the session.
pub(crate) fn lumen_idb_dir() -> Option<std::path::PathBuf> {
    let dir = adblock::browser_data_dir().join("idb");

    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("idb: не удалось создать директорию {}: {e}", dir.display());
        return None;
    }
    Some(dir)
}

/// Get-or-create the localStorage partition for the given `ResourceBase` origin.
/// Returns `None` for file: bases (no persistent origin-partitioned storage).
pub(crate) fn ls_store_for_base(
    base: &ResourceBase,
    ls_storage: &mut HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
) -> Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>> {
    let origin = storage_origin_for_base(base)?;
    Some(Arc::clone(ls_storage.entry(origin).or_insert_with(|| {
        Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
    })))
}

/// The `sessionStorage` partition for `base` (BUG-836).
///
/// Same origin keying as [`ls_store_for_base`]; the difference is the map it
/// draws from — `ss_storage` is per *tab* (cleared when a tab is opened, carried
/// in the tab snapshot), so the store survives navigation within the tab and
/// nothing else, as HTML LS §12.2 requires.
pub(crate) fn ss_store_for_base(
    base: &ResourceBase,
    ss_storage: &mut HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
) -> Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>> {
    let origin = storage_origin_for_base(base)?;
    Some(Arc::clone(ss_storage.entry(origin).or_insert_with(|| {
        Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
    })))
}

/// Origin key (`scheme://host[:port]`) used to partition Web Storage.
///
/// `None` for a `file:` base — an opaque origin gets no storage at all.
pub(crate) fn storage_origin_for_base(base: &ResourceBase) -> Option<String> {
    match base {
        ResourceBase::Url(u) => lumen_core::url::Url::parse(u).ok().map(|parsed| {
            let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", parsed.scheme(), parsed.host(), port)
        }),
        ResourceBase::File(_) => None,
    }
}

/// Build the per-origin IndexedDB persistence handle for the given `ResourceBase`.
///
/// Returns `None` for `file:` bases (no origin storage).
/// When `idb_dir` is `Some`, opens or creates a dedicated SQLite file
/// `{idb_dir}/{sha256_hex(eTLD+1)[:16]}.db`; when `None` uses an ephemeral
/// in-memory store (tests / headless — no cross-reload persistence).
pub(crate) fn idb_store_for_base(
    base: &ResourceBase,
    idb_dir: Option<&std::path::Path>,
) -> Option<Arc<dyn lumen_core::ext::IdbBackend>> {
    let url = match base {
        ResourceBase::Url(u) => u.as_str(),
        ResourceBase::File(_) => return None,
    };
    idb_store_for_url(url, idb_dir)
}

/// Core IDB store builder — shared by [`idb_store_for_base`] and the reload path.
pub(crate) fn idb_store_for_url(
    url: &str,
    idb_dir: Option<&std::path::Path>,
) -> Option<Arc<dyn lumen_core::ext::IdbBackend>> {
    let parsed = lumen_core::url::Url::parse(url).ok()?;
    let host = parsed.host();
    if host.is_empty() {
        return None;
    }
    // eTLD+1 for key derivation; falls back to raw host (IPs, localhost, unknown TLDs).
    let etld_plus_one = {
        use lumen_core::ext::PublicSuffixList;
        lumen_storage::PslProvider::new()
            .registrable_domain(host)
            .unwrap_or(host)
            .to_string()
    };
    if let Some(dir) = idb_dir {
        // Phase 3: structured per-origin SQLite backend (idb_meta/stores/indexes/records
        // + snapshot-blob fallback). Falls back to ephemeral in-memory below when no dir.
        lumen_storage::NativeIdbStore::for_origin(&etld_plus_one, dir).ok()
    } else {
        let origin = format!("{}://{}", parsed.scheme(), parsed.host());
        Some(Arc::new(lumen_storage::IdbStore::new(
            Arc::new(Mutex::new(lumen_storage::store::InMemoryStorage::new())),
            origin,
        )))
    }
}

/// Build the per-origin Service Worker registration persistence handle for the
/// given `ResourceBase`. Returns `None` for `file:` bases (no persistent storage).
/// The returned `SwStore` shares `backend`, so SW registrations survive page reloads.
pub(crate) fn sw_store_for_base(
    base: &ResourceBase,
    backend: &Arc<std::sync::Mutex<dyn lumen_core::ext::StorageBackend>>,
    registry: Option<&Arc<lumen_storage::ServiceWorkers>>,
) -> Option<Arc<dyn lumen_core::ext::SwBackend>> {
    let origin = match base {
        ResourceBase::Url(u) => lumen_core::url::Url::parse(u).ok().map(|parsed| {
            let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", parsed.scheme(), parsed.host(), port)
        })?,
        ResourceBase::File(_) => return None,
    };
    let store = lumen_storage::SwStore::new(Arc::clone(backend), origin.clone());
    Some(match registry {
        Some(reg) => Arc::new(SwRegistryMirror { inner: store, registry: Arc::clone(reg), origin }),
        None => Arc::new(store),
    })
}

/// `SwBackend`, который помимо снимка регистраций кладёт их в SQLite-реестр
/// профиля (`ServiceWorkers`), чтобы «Очистить данные сайта» (UX-PARTITION)
/// видел то, что страница зарегистрировала.
struct SwRegistryMirror {
    inner: lumen_storage::SwStore,
    registry: Arc<lumen_storage::ServiceWorkers>,
    origin: String,
}

impl lumen_core::ext::SwBackend for SwRegistryMirror {
    fn load(&self) -> Option<String> {
        self.inner.load()
    }

    fn save(&self, snapshot: &str) {
        self.inner.save(snapshot);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        sync_sw_registry(&self.registry, &self.origin, snapshot, now);
    }
}

/// Привести записи `origin` в реестре к JSON-снимку `[{scope, scriptURL}]`:
/// новые регистрации вставляются, исчезнувшие — удаляются. Best-effort.
fn sync_sw_registry(registry: &lumen_storage::ServiceWorkers, origin: &str, snapshot: &str, now: i64) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(snapshot) else {
        return;
    };
    let mut live = std::collections::HashSet::new();
    for item in &items {
        let (Some(scope), Some(script)) = (
            item.get("scope").and_then(|v| v.as_str()),
            item.get("scriptURL").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        live.insert(scope.to_string());
        let _ = registry.register(origin, scope, script, lumen_storage::UpdateViaCache::Imports, now);
    }
    if let Ok(existing) = registry.list_for_origin(origin) {
        for reg in existing.into_iter().filter(|r| !live.contains(&r.scope)) {
            let _ = registry.unregister(reg.id);
        }
    }
}

#[cfg(test)]
mod sw_registry_tests {
    use super::*;

    #[test]
    fn snapshot_is_mirrored_and_pruned() {
        let reg = lumen_storage::ServiceWorkers::open_in_memory().unwrap();
        let o = "https://a.example";
        sync_sw_registry(&reg, o, r#"[{"scope":"/","scriptURL":"https://a.example/sw.js","state":"activated"},{"scope":"/x/","scriptURL":"https://a.example/x.js","state":"installing"}]"#, 10);
        assert_eq!(reg.list_for_origin(o).unwrap().len(), 2);
        sync_sw_registry(&reg, o, r#"[{"scope":"/","scriptURL":"https://a.example/sw2.js","state":"activated"}]"#, 11);
        let l = reg.list_for_origin(o).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].script_url, "https://a.example/sw2.js");
        sync_sw_registry(&reg, o, "garbage", 12);
        assert_eq!(reg.count().unwrap(), 1);
    }
}
