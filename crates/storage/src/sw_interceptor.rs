//! Service Worker fetch interception — SQLite-backed реализация
//! `lumen_core::ext::FetchInterceptor`.
//!
//! При каждом fetch: (1) проверяем, есть ли SW с scope, покрывающим URL;
//! (2) если есть — ищем ответ в CacheStorage любого именованного кэша для
//! этого origin. Возвращаем первое найденное тело. Если SW нет или кэш
//! пуст — возвращаем None, запрос уходит в сеть штатно.
//!
//! Имя кэша не указывается явно: interceptor проверяет все кэши origin-а
//! (как `caches.match()` без имени в JS). Для GET-запросов (ответ из кэша
//! всегда GET).

use std::sync::Arc;

use lumen_core::ext::FetchInterceptor;
use lumen_core::url::Url;

use crate::cache_storage::CacheStorage;
use crate::service_workers::ServiceWorkers;

/// SQLite-backed SW fetch interceptor.
///
/// Проверяет SW-регистрации и CacheStorage при каждом fetch из lumen-network.
/// Конструируется в shell и передаётся в `HttpClient::with_interceptor()`.
pub struct ServiceWorkerInterceptor {
    pub sw_store: Arc<ServiceWorkers>,
    pub cache_store: Arc<CacheStorage>,
}

impl ServiceWorkerInterceptor {
    pub fn new(sw_store: Arc<ServiceWorkers>, cache_store: Arc<CacheStorage>) -> Self {
        Self {
            sw_store,
            cache_store,
        }
    }
}

impl FetchInterceptor for ServiceWorkerInterceptor {
    fn intercept(&self, url: &Url, origin: &str) -> Option<Vec<u8>> {
        // 1. Есть ли SW, scope которого покрывает путь этого URL?
        // find_for_url ожидает path (как /app/page), не полный URL.
        let _reg = self
            .sw_store
            .find_for_url(origin, url.path())
            .ok()??;

        // 2. Проверяем все именованные кэши origin-а (как caches.match()).
        // CacheStorage хранит полный URL в request_url.
        let cache_names = self.cache_store.list_cache_names(origin).ok()?;
        for name in &cache_names {
            if let Ok(Some(entry)) =
                self.cache_store.match_(origin, name, url.as_str(), "GET")
            {
                return Some(entry.response_body);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_storage::CacheStorage;
    use crate::service_workers::{ServiceWorkers, UpdateViaCache};

    fn make() -> ServiceWorkerInterceptor {
        ServiceWorkerInterceptor::new(
            Arc::new(ServiceWorkers::open_in_memory().unwrap()),
            Arc::new(CacheStorage::open_in_memory().unwrap()),
        )
    }

    #[test]
    fn no_sw_registered_returns_none() {
        let si = make();
        let url = Url::parse("https://example.com/page").unwrap();
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn sw_registered_but_cache_empty_returns_none() {
        let si = make();
        si.sw_store
            .register(
                "https://example.com",
                "/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/api/data").unwrap();
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn sw_registered_cache_hit_returns_body() {
        let si = make();
        si.sw_store
            .register(
                "https://example.com",
                "/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        si.cache_store
            .put(
                "https://example.com",
                "v1",
                "https://example.com/api/data",
                "GET",
                200,
                "",
                b"cached body",
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/api/data").unwrap();
        let result = si.intercept(&url, "https://example.com");
        assert_eq!(result, Some(b"cached body".to_vec()));
    }

    #[test]
    fn sw_scope_mismatch_returns_none() {
        let si = make();
        // SW покрывает только /app/, а запрос к /other/
        si.sw_store
            .register(
                "https://example.com",
                "/app/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        si.cache_store
            .put(
                "https://example.com",
                "v1",
                "https://example.com/other/page",
                "GET",
                200,
                "",
                b"body",
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/other/page").unwrap();
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn different_origin_sw_does_not_intercept() {
        let si = make();
        si.sw_store
            .register(
                "https://other.com",
                "/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        si.cache_store
            .put(
                "https://other.com",
                "v1",
                "https://example.com/page",
                "GET",
                200,
                "",
                b"body",
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/page").unwrap();
        // origin=example.com, SW есть только для other.com
        assert!(si.intercept(&url, "https://example.com").is_none());
    }
}
