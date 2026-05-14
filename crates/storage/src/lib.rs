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
pub mod bookmarks;
pub mod cookies;
pub mod dns_cache;
pub mod downloads;
pub mod history;
pub mod http_cache;
pub mod notifications;
pub mod permissions;
pub mod plugins;
pub mod profiles;
pub mod push_subscriptions;
pub mod referrer_policy;
pub mod search_history;
pub mod search_providers;
pub mod service_workers;
pub mod site_engagement;
pub mod sqlite_store;
pub mod store;
pub mod tab_sessions;
pub mod web_manifest;
pub mod workspaces;

pub use autofill::{Autofill, AutofillEntry};
pub use bookmarks::{Bookmark, Bookmarks};
pub use cookies::{parse_set_cookie, Cookie, CookieJar, SameSite};
pub use dns_cache::{DnsCache, DnsEntry};
pub use downloads::{DownloadEntry, DownloadStatus, Downloads};
pub use history::{History, HistoryEntry};
pub use http_cache::{CacheControl, CachedResponse, HttpCache};
pub use notifications::{Notification, Notifications};
pub use permissions::{PermissionEntry, PermissionKind, PermissionState, Permissions};
pub use plugins::{PluginManifest, Plugins};
pub use profiles::{Profile, ProfileRegistry};
pub use push_subscriptions::{PushSubscription, PushSubscriptions};
pub use referrer_policy::{ReferrerPolicies, ReferrerPolicy};
pub use search_history::{SearchHistory, SearchQuery};
pub use search_providers::{SearchProviderEntry, SearchProviders};
pub use service_workers::{ServiceWorkerRegistration, ServiceWorkers, UpdateViaCache};
pub use site_engagement::{SiteEngagement, SiteEngagementStore};
pub use sqlite_store::SqliteStorage;
pub use store::InMemoryStorage;
pub use tab_sessions::{SessionSnapshot, TabSession, TabSessions};
pub use web_manifest::{WebManifest, WebManifests};
pub use workspaces::{Workspace, Workspaces};
