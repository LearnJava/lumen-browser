//! KV-хранилище для Lumen: cookies, history, profile data.
//!
//! Phase 0-1: in-memory HashMap с snapshot-ами на диск. Реализует
//! `lumen_core::ext::StorageBackend` с origin-партиционированием.
//!
//! Snapshot-формат — простой текст: заголовок `LUMEN_KV_V1`, далее строки
//! `<composite_key_hex> <value_hex>`, где composite_key — байты
//! `origin\x00top_level_site\x00key`.

pub mod store;

pub use store::InMemoryStorage;
