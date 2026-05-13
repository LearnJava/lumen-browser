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

pub mod bookmarks;
pub mod cookies;
pub mod history;
pub mod http_cache;
pub mod profiles;
pub mod sqlite_store;
pub mod store;

pub use bookmarks::{Bookmark, Bookmarks};
pub use cookies::{parse_set_cookie, Cookie, CookieJar, SameSite};
pub use history::{History, HistoryEntry};
pub use http_cache::{CacheControl, CachedResponse, HttpCache};
pub use profiles::{Profile, ProfileRegistry};
pub use sqlite_store::SqliteStorage;
pub use store::InMemoryStorage;
