//! HSTS Preload List (RFC 8838): встроенный список доменов, требующих HTTPS.
//!
//! Источник: Chromium HSTS Preload List. Автоматически обновляется раз в месяц.
//! Специфика: не зависит от HSTS-header'ов; загружается при старте и проверяется
//! перед каждым HTTP-запросом для upgrade HTTP→HTTPS без редиректа.
//!
//! Алгоритм: `is_preloaded(host)` преобразует host в eTLD+1 (например,
//! `sub.example.com` → `example.com`), ищет в preload list и проверяет флаг
//! `include_subdomains`. Если домен в list и include_subdomains=true, то
//! даже поддомены получают upgrade.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Запись в HSTS Preload List.
#[derive(Debug, Clone)]
struct HstsPreloadEntry {
    /// Если true, то поддомены тоже требуют HTTPS.
    include_subdomains: bool,
}

/// HSTS Preload List: быстрый поиск по eTLD+1.
pub struct HstsPreloadList {
    entries: HashMap<String, HstsPreloadEntry>,
}

impl HstsPreloadList {
    /// Создать preload list из встроенного JSON (Chromium формат).
    /// Парсит массив вида:
    /// ```json
    /// [
    ///   {"name": "example.com", "include_subdomains": true},
    ///   {"name": "github.com", "include_subdomains": false}
    /// ]
    /// ```
    pub fn load() -> Self {
        let json = include_str!("../assets/hsts_preload.json");
        let mut entries = HashMap::new();

        // Простой JSON парсер (не используем serde для минимизации зависимостей).
        // Формат: [{"name":"...","include_subdomains":...},...].
        let data = json.trim();
        if !data.starts_with('[') || !data.ends_with(']') {
            return HstsPreloadList { entries };
        }

        let content = &data[1..data.len() - 1];
        for item_str in content.split('}') {
            if item_str.is_empty() {
                continue;
            }

            let item = item_str.trim();
            let item = if let Some(stripped) = item.strip_prefix(',') {
                stripped.trim()
            } else {
                item
            };

            if !item.contains("\"name\"") {
                continue;
            }

            // Парсим "name": "example.com"
            let name = if let Some(start) = item.find("\"name\":") {
                let rest = &item[start + 7..];
                if let Some(quote_start) = rest.find('"') {
                    let rest = &rest[quote_start + 1..];
                    if let Some(quote_end) = rest.find('"') {
                        rest[..quote_end].to_string()
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            } else {
                continue;
            };

            // Парсим "include_subdomains": true/false
            let include_subdomains = item.contains("\"include_subdomains\":true");

            entries.insert(name, HstsPreloadEntry {
                include_subdomains,
            });
        }

        HstsPreloadList { entries }
    }

    /// Проверить, есть ли хост в preload list.
    /// Возвращает true если хост или его eTLD+1 в list и include_subdomains активен.
    ///
    /// Примеры:
    /// - Если `github.com` в list с include_subdomains=true, то
    ///   `is_preloaded("sub.github.com")` вернёт true.
    /// - Если `example.com` в list с include_subdomains=false, то
    ///   `is_preloaded("sub.example.com")` вернёт false.
    pub fn is_preloaded(&self, host: &str) -> bool {
        let host = host.trim().to_lowercase();

        // Простая эвристика для eTLD+1: берём последние 2 компоненты.
        // (Правильный алгоритм требует Public Suffix List, но для MVP этого достаточно.)
        let etld_plus_one = if let Some(last_dot) = host.rfind('.') {
            if let Some(prev_dot) = host[..last_dot].rfind('.') {
                host[prev_dot + 1..].to_string()
            } else {
                host.clone()
            }
        } else {
            host.clone()
        };

        // Поиск в list
        if let Some(entry) = self.entries.get(&etld_plus_one) {
            return entry.include_subdomains || host == etld_plus_one;
        }

        false
    }
}

/// Глобальный экземпляр preload list (ленивая инициализация).
static PRELOAD_LIST: OnceLock<HstsPreloadList> = OnceLock::new();

/// Получить глобальный preload list.
pub fn get_preload_list() -> &'static HstsPreloadList {
    PRELOAD_LIST.get_or_init(HstsPreloadList::load)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preload_list_load() {
        let list = HstsPreloadList::load();
        // После того как загружу JSON, проверю, что entries непусто.
        assert!(!list.entries.is_empty(), "preload list should not be empty");
    }

    #[test]
    fn test_is_preloaded_exact_match() {
        let list = HstsPreloadList::load();
        // Проверяю, что github.com в list (популярный домен).
        assert!(list.is_preloaded("github.com"));
    }

    #[test]
    fn test_is_preloaded_subdomain_no_subdomains_flag() {
        let mut entries = HashMap::new();
        entries.insert(
            "example.com".to_string(),
            HstsPreloadEntry {
                include_subdomains: false,
            },
        );
        let list = HstsPreloadList { entries };

        assert!(list.is_preloaded("example.com"));
        assert!(!list.is_preloaded("sub.example.com"));
    }

    #[test]
    fn test_is_preloaded_subdomain_with_subdomains_flag() {
        let mut entries = HashMap::new();
        entries.insert(
            "github.com".to_string(),
            HstsPreloadEntry {
                include_subdomains: true,
            },
        );
        let list = HstsPreloadList { entries };

        assert!(list.is_preloaded("github.com"));
        assert!(list.is_preloaded("sub.github.com"));
        assert!(list.is_preloaded("deep.sub.github.com"));
    }

    #[test]
    fn test_is_preloaded_case_insensitive() {
        let mut entries = HashMap::new();
        entries.insert(
            "github.com".to_string(),
            HstsPreloadEntry {
                include_subdomains: true,
            },
        );
        let list = HstsPreloadList { entries };

        assert!(list.is_preloaded("GITHUB.COM"));
        assert!(list.is_preloaded("Github.Com"));
    }

    #[test]
    fn test_is_preloaded_not_in_list() {
        let entries = HashMap::new();
        let list = HstsPreloadList { entries };

        assert!(!list.is_preloaded("notinlist.com"));
    }
}
