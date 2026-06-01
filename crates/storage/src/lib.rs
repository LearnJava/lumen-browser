//! KV-хранилище для Lumen: cookies, history, profile data.
//!
//! Два бэкенда, реализующих `lumen_core::ext::StorageBackend`:
//! - [`InMemoryStorage`] — in-memory `HashMap` с snapshot-ами `LUMEN_KV_V1`
//!   на диск. Подходит для тестов и ephemeral session-state.
//! - [`SqliteStorage`] — persistent SQLite (exception #5 в §5). Подходит
//!   для history, bookmarks, notes, cookies-TTL, профилей — всего, что
//!   должно пережить рестарт. Создаёт одну таблицу `kv` с составным
//!   первичным ключом `(origin, top_level_site, key)`; WAL + synchronous
//!   NORMAL по умолчанию.
//!
//! Оба бэкенда соблюдают одинаковую семантику origin-партиционирования
//! (`None` и `Some("")` — один namespace) и реализуют тот же trait.

pub mod autofill;
pub mod bfcache;
pub mod bookmarks;
pub mod broadcast_channels;
pub mod cache_storage;
pub mod cached_dns;
pub mod cookies;
pub mod csp_policies;
pub mod dns_cache;
pub mod downloads;
pub mod history;
pub mod hsts;
pub mod http_cache;
pub mod indexed_db;
pub mod notifications;
pub mod permissions;
pub mod permissions_policy;
pub mod plugins;
pub mod profiles;
pub mod psl;
pub mod push_subscriptions;
pub mod referrer_policy;
pub mod safe_browsing;
pub mod search_history;
pub mod session_export;
pub mod search_providers;
pub mod service_workers;
pub mod site_engagement;
pub mod sw_interceptor;
pub mod sw_store;
pub mod sqlite_store;
pub mod store;
pub mod tab_sessions;
pub mod tab_snapshot;
pub mod web_manifest;
pub mod workspaces;

pub use autofill::{Autofill, AutofillEntry};
pub use bfcache::{BfCache, BfCacheEntry};
pub use bookmarks::{Bookmark, Bookmarks};
pub use broadcast_channels::{BroadcastChannels, ChannelRegistration};
pub use cache_storage::{CacheStorage, CachedEntry};
pub use cookies::{parse_set_cookie, parse_set_cookie_with_psl, Cookie, CookieJar, CookieJarProvider, SameSite};
pub use csp_policies::{parse_csp_header, CspPolicies, CspPolicy};
pub use hsts::{parse_sts_header, HstsEntry, HstsStore};
pub use cached_dns::{CachedDnsResolver, Clock, SystemClock};
pub use dns_cache::{DnsCache, DnsEntry};
pub use downloads::{DownloadEntry, DownloadStatus, Downloads};
pub use history::{History, HistoryEntry};
pub use http_cache::{CacheControl, CachedResponse, HttpCache};
pub use indexed_db::IdbStore;
pub use notifications::{Notification, Notifications};
pub use permissions::{PermissionEntry, PermissionKind, PermissionState, Permissions};
pub use permissions_policy::{
    parse_permissions_policy, PermissionsAllowlist, PermissionsPolicies, PermissionsPolicy,
};
pub use plugins::{PluginManifest, Plugins};
pub use profiles::{Profile, ProfileRegistry};
pub use psl::PslProvider;
pub use push_subscriptions::{PushSubscription, PushSubscriptions};
pub use referrer_policy::{ReferrerPolicies, ReferrerPolicy};
pub use safe_browsing::{
    SafeBrowsingFilter, SafeBrowsingList, ThreatType, canonical_expression_variants,
    canonical_expression_variants_with_psl, hash_expression,
};
pub use search_history::{SearchHistory, SearchQuery};
pub use search_providers::{SearchProviderEntry, SearchProviders};
pub use service_workers::{ServiceWorkerRegistration, ServiceWorkers, UpdateViaCache};
pub use sw_interceptor::ServiceWorkerInterceptor;
pub use sw_store::SwStore;
pub use site_engagement::{SiteEngagement, SiteEngagementStore};
pub use sqlite_store::SqliteStorage;
pub use store::InMemoryStorage;
pub use session_export::{active_tab, from_json as session_from_json, to_json as session_to_json,
    ExportedTab, SessionFile};
pub use tab_sessions::{SessionSnapshot, TabSession, TabSessions};
pub use tab_snapshot::{HibernatedTabData, TabSnapshotStore};
pub use web_manifest::{WebManifest, WebManifests};
pub use workspaces::{Workspace, Workspaces};
