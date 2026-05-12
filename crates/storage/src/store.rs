//! In-memory KV-хранилище с origin-партиционированием и snapshot-ами.

use std::collections::HashMap;
use std::path::Path;

use lumen_core::ext::StorageBackend;
use lumen_core::{Error, Result};

/// In-memory KV-хранилище. Все данные в RAM; `serialize`/`deserialize`
/// дают байтовый snapshot для сохранения на диск.
#[derive(Debug, Default)]
pub struct InMemoryStorage {
    data: HashMap<PartitionedKey, Vec<u8>>,
}

/// Составной ключ с origin-партиционированием.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PartitionedKey {
    origin: String,
    top_level_site: String,
    key: String,
}

impl PartitionedKey {
    fn new(origin: Option<&str>, top_level_site: Option<&str>, key: &str) -> Self {
        Self {
            origin: origin.unwrap_or("").to_string(),
            top_level_site: top_level_site.unwrap_or("").to_string(),
            key: key.to_string(),
        }
    }

    /// Кодирует ключ в байты: `origin\x00top_level_site\x00key`.
    fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(self.origin.as_bytes());
        v.push(0);
        v.extend_from_slice(self.top_level_site.as_bytes());
        v.push(0);
        v.extend_from_slice(self.key.as_bytes());
        v
    }

    /// Разбирает байты обратно в PartitionedKey.
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut parts = bytes.splitn(3, |&b| b == 0);
        let origin = std::str::from_utf8(parts.next()?).ok()?;
        let top_level_site = std::str::from_utf8(parts.next()?).ok()?;
        let key = std::str::from_utf8(parts.next()?).ok()?;
        Some(Self {
            origin: origin.to_string(),
            top_level_site: top_level_site.to_string(),
            key: key.to_string(),
        })
    }
}

// ── Snapshot I/O ─────────────────────────────────────────────────────────────

/// `LUMEN_KV_V1\n<key_hex> <value_hex>\n...`
const HEADER: &str = "LUMEN_KV_V1";

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok())
        .collect()
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Сериализует хранилище в байты (snapshot-формат `LUMEN_KV_V1`).
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = String::from(HEADER);
        out.push('\n');
        for (pk, value) in &self.data {
            out.push_str(&to_hex(&pk.to_bytes()));
            out.push(' ');
            out.push_str(&to_hex(value));
            out.push('\n');
        }
        out.into_bytes()
    }

    /// Десериализует snapshot.
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Parse("snapshot: invalid UTF-8".into()))?;
        let mut lines = text.lines();

        let header = lines
            .next()
            .ok_or_else(|| Error::Parse("snapshot: empty file".into()))?;
        if header != HEADER {
            return Err(Error::Parse(format!(
                "snapshot: unknown format '{header}'"
            )));
        }

        let mut data = HashMap::new();
        for (i, line) in lines.enumerate() {
            if line.is_empty() {
                continue;
            }
            let (key_hex, val_hex) = line.split_once(' ').ok_or_else(|| {
                Error::Parse(format!("snapshot line {}: missing space separator", i + 2))
            })?;
            let key_bytes = from_hex(key_hex).ok_or_else(|| {
                Error::Parse(format!("snapshot line {}: bad key hex", i + 2))
            })?;
            let value = from_hex(val_hex).ok_or_else(|| {
                Error::Parse(format!("snapshot line {}: bad value hex", i + 2))
            })?;
            let pk = PartitionedKey::from_bytes(&key_bytes).ok_or_else(|| {
                Error::Parse(format!("snapshot line {}: bad composite key", i + 2))
            })?;
            data.insert(pk, value);
        }

        Ok(Self { data })
    }

    /// Сохраняет snapshot в файл.
    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.serialize())
            .map_err(|e| Error::Io(e.to_string()))
    }

    /// Загружает snapshot из файла.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io(e.to_string()))?;
        Self::deserialize(&bytes)
    }
}

// ── StorageBackend impl ───────────────────────────────────────────────────────

impl StorageBackend for InMemoryStorage {
    fn get(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(self.data.get(&PartitionedKey::new(origin, top_level_site, key)).cloned())
    }

    fn put(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
        value: &[u8],
    ) -> Result<()> {
        self.data.insert(PartitionedKey::new(origin, top_level_site, key), value.to_vec());
        Ok(())
    }

    fn delete(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<()> {
        self.data.remove(&PartitionedKey::new(origin, top_level_site, key));
        Ok(())
    }

    fn list_keys(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
    ) -> Result<Vec<String>> {
        let origin = origin.unwrap_or("");
        let tls = top_level_site.unwrap_or("");
        Ok(self
            .data
            .keys()
            .filter(|pk| pk.origin == origin && pk.top_level_site == tls)
            .map(|pk| pk.key.clone())
            .collect())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── CRUD ─────────────────────────────────────────────────────────────────

    #[test]
    fn put_then_get_returns_value() {
        let mut s = InMemoryStorage::new();
        s.put(None, None, "hello", b"world").unwrap();
        assert_eq!(s.get(None, None, "hello").unwrap(), Some(b"world".to_vec()));
    }

    #[test]
    fn get_missing_returns_none() {
        let s = InMemoryStorage::new();
        assert_eq!(s.get(None, None, "nope").unwrap(), None);
    }

    #[test]
    fn delete_removes_entry() {
        let mut s = InMemoryStorage::new();
        s.put(None, None, "x", b"v").unwrap();
        s.delete(None, None, "x").unwrap();
        assert_eq!(s.get(None, None, "x").unwrap(), None);
    }

    #[test]
    fn delete_nonexistent_is_ok() {
        let mut s = InMemoryStorage::new();
        assert!(s.delete(None, None, "ghost").is_ok());
    }

    #[test]
    fn put_overwrites() {
        let mut s = InMemoryStorage::new();
        s.put(None, None, "k", b"v1").unwrap();
        s.put(None, None, "k", b"v2").unwrap();
        assert_eq!(s.get(None, None, "k").unwrap(), Some(b"v2".to_vec()));
    }

    // ── Origin-партиционирование ──────────────────────────────────────────────

    #[test]
    fn origin_partitions_keys() {
        let mut s = InMemoryStorage::new();
        s.put(Some("example.com"), None, "token", b"abc").unwrap();
        s.put(Some("other.com"), None, "token", b"xyz").unwrap();
        // Каждый origin видит только свои данные.
        assert_eq!(
            s.get(Some("example.com"), None, "token").unwrap(),
            Some(b"abc".to_vec())
        );
        assert_eq!(
            s.get(Some("other.com"), None, "token").unwrap(),
            Some(b"xyz".to_vec())
        );
        // Глобальный namespace пуст.
        assert_eq!(s.get(None, None, "token").unwrap(), None);
    }

    #[test]
    fn top_level_site_partitions() {
        let mut s = InMemoryStorage::new();
        s.put(Some("ads.com"), Some("site1.com"), "track", b"1").unwrap();
        s.put(Some("ads.com"), Some("site2.com"), "track", b"2").unwrap();
        assert_eq!(
            s.get(Some("ads.com"), Some("site1.com"), "track").unwrap(),
            Some(b"1".to_vec())
        );
        assert_eq!(
            s.get(Some("ads.com"), Some("site2.com"), "track").unwrap(),
            Some(b"2".to_vec())
        );
    }

    #[test]
    fn none_and_empty_origin_are_same_partition() {
        let mut s = InMemoryStorage::new();
        s.put(None, None, "k", b"v").unwrap();
        // None и "" — один и тот же partition (глобальный).
        assert_eq!(s.get(Some(""), Some(""), "k").unwrap(), Some(b"v".to_vec()));
    }

    // ── list_keys ────────────────────────────────────────────────────────────

    #[test]
    fn list_keys_returns_keys_in_partition() {
        let mut s = InMemoryStorage::new();
        s.put(None, None, "a", b"1").unwrap();
        s.put(None, None, "b", b"2").unwrap();
        let mut keys = s.list_keys(None, None).unwrap();
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn list_keys_isolates_by_origin() {
        let mut s = InMemoryStorage::new();
        s.put(Some("a.com"), None, "k1", b"v").unwrap();
        s.put(Some("b.com"), None, "k2", b"v").unwrap();
        let keys = s.list_keys(Some("a.com"), None).unwrap();
        assert_eq!(keys, vec!["k1"]);
    }

    #[test]
    fn list_keys_empty_when_no_entries() {
        let s = InMemoryStorage::new();
        assert!(s.list_keys(None, None).unwrap().is_empty());
    }

    // ── Snapshot ─────────────────────────────────────────────────────────────

    #[test]
    fn snapshot_empty_round_trip() {
        let s = InMemoryStorage::new();
        let bytes = s.serialize();
        let s2 = InMemoryStorage::deserialize(&bytes).unwrap();
        assert!(s2.list_keys(None, None).unwrap().is_empty());
    }

    #[test]
    fn snapshot_entries_round_trip() {
        let mut s = InMemoryStorage::new();
        s.put(None, None, "key1", b"val1").unwrap();
        s.put(Some("example.com"), None, "session", b"tok123").unwrap();
        s.put(Some("a.com"), Some("b.com"), "c", b"d").unwrap();

        let s2 = InMemoryStorage::deserialize(&s.serialize()).unwrap();

        assert_eq!(s2.get(None, None, "key1").unwrap(), Some(b"val1".to_vec()));
        assert_eq!(
            s2.get(Some("example.com"), None, "session").unwrap(),
            Some(b"tok123".to_vec())
        );
        assert_eq!(s2.get(Some("a.com"), Some("b.com"), "c").unwrap(), Some(b"d".to_vec()));
    }

    #[test]
    fn snapshot_binary_values_round_trip() {
        let mut s = InMemoryStorage::new();
        let binary: Vec<u8> = (0u8..=255).collect();
        s.put(Some("x.com"), None, "bin", &binary).unwrap();

        let s2 = InMemoryStorage::deserialize(&s.serialize()).unwrap();
        assert_eq!(s2.get(Some("x.com"), None, "bin").unwrap(), Some(binary));
    }

    #[test]
    fn snapshot_cyrillic_keys_and_values() {
        let mut s = InMemoryStorage::new();
        s.put(Some("кириллица.рф"), None, "ключ", "значение".as_bytes()).unwrap();

        let s2 = InMemoryStorage::deserialize(&s.serialize()).unwrap();
        assert_eq!(
            s2.get(Some("кириллица.рф"), None, "ключ").unwrap(),
            Some("значение".as_bytes().to_vec())
        );
    }

    #[test]
    fn snapshot_bad_header_returns_error() {
        let result = InMemoryStorage::deserialize(b"BAD_HEADER\n");
        assert!(result.is_err());
    }

    #[test]
    fn snapshot_empty_bytes_returns_error() {
        let result = InMemoryStorage::deserialize(b"");
        assert!(result.is_err());
    }
}
