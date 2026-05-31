//! Mock HTTP транспорт для тестирования.
//!
//! Реализует `NetworkTransport`, но вместо реальных HTTP-запросов возвращает
//! заранее зарегистрированные fixture-данные. Используется для изолированного
//! тестирования, когда нужно избежать реальных сетевых запросов.
//!
//! # Пример
//! ```rust,no_run
//! use lumen_network::MockTransport;
//! use lumen_core::ext::NetworkTransport;
//! use lumen_core::url::Url;
//!
//! let mut transport = MockTransport::new();
//! transport.add_fixture("http://example.com/page.html", b"<html>test</html>".to_vec());
//!
//! let url = Url::parse("http://example.com/page.html").unwrap();
//! let data = transport.fetch(&url).unwrap();
//! assert_eq!(data, b"<html>test</html>");
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lumen_core::error::{Error, Result};
use lumen_core::ext::NetworkTransport;
use lumen_core::url::Url;

/// Mock HTTP транспорт — перехватывает запросы и возвращает fixture-данные.
///
/// Применяется для тестирования в изолированной среде без реальных сетевых
/// запросов. Фиксатуры (содержимое HTTP-ответов) регистрируются по полному
/// URL через `add_fixture()` перед использованием транспорта.
pub struct MockTransport {
    fixtures: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockTransport {
    /// Создать пустой mock транспорт без зарегистрированных фиксатур.
    pub fn new() -> Self {
        Self {
            fixtures: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Зарегистрировать fixture-данные для URL.
    ///
    /// При последующем вызове `fetch()` с этим URL будут возвращены
    /// переданные данные.
    ///
    /// # Параметры
    /// * `url` — полный URL (строка или преобразуемое в String).
    /// * `data` — содержимое, которое должен вернуть `fetch()`.
    pub fn add_fixture(&mut self, url: impl Into<String>, data: Vec<u8>) {
        let url_str = url.into();
        if let Ok(mut fixtures) = self.fixtures.lock() {
            fixtures.insert(url_str, data);
        }
    }

    /// Получить текущее количество зарегистрированных фиксатур.
    ///
    /// Полезно для отладки и валидации в тестах.
    pub fn fixture_count(&self) -> usize {
        self.fixtures
            .lock()
            .map(|f| f.len())
            .unwrap_or(0)
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MockTransport {
    /// Клонировать транспорт — разделить одну и ту же таблицу фиксатур.
    ///
    /// Клоны видят друг друга фиксатуры через `Arc<Mutex<>>`.
    fn clone(&self) -> Self {
        Self {
            fixtures: Arc::clone(&self.fixtures),
        }
    }
}

impl NetworkTransport for MockTransport {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>> {
        let url_str = url.as_str();
        let fixtures = self
            .fixtures
            .lock()
            .map_err(|e| Error::Network(format!("mutex lock failed: {e}")))?;

        fixtures
            .get(url_str)
            .cloned()
            .ok_or_else(|| {
                Error::Network(format!(
                    "No fixture registered for URL: {}. Available: [{}]",
                    url_str,
                    fixtures
                        .keys()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_fetch_fixture() {
        let mut transport = MockTransport::new();
        transport.add_fixture("http://example.com/page.html", b"<html>test</html>".to_vec());

        let url = Url::parse("http://example.com/page.html").unwrap();
        let data = transport.fetch(&url).unwrap();
        assert_eq!(data, b"<html>test</html>");
    }

    #[test]
    fn test_missing_fixture_error() {
        let transport = MockTransport::new();
        let url = Url::parse("http://example.com/missing.html").unwrap();

        let result = transport.fetch(&url);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("No fixture registered"));
    }

    #[test]
    fn test_multiple_fixtures() {
        let mut transport = MockTransport::new();
        transport.add_fixture("http://example.com/page1.html", b"page1".to_vec());
        transport.add_fixture("http://example.com/page2.html", b"page2".to_vec());

        assert_eq!(transport.fixture_count(), 2);

        let url1 = Url::parse("http://example.com/page1.html").unwrap();
        assert_eq!(transport.fetch(&url1).unwrap(), b"page1");

        let url2 = Url::parse("http://example.com/page2.html").unwrap();
        assert_eq!(transport.fetch(&url2).unwrap(), b"page2");
    }

    #[test]
    fn test_clone_shares_fixtures() {
        let mut transport = MockTransport::new();
        transport.add_fixture("http://example.com/page.html", b"shared".to_vec());

        let cloned = transport.clone();
        let url = Url::parse("http://example.com/page.html").unwrap();
        assert_eq!(cloned.fetch(&url).unwrap(), b"shared");
    }

    #[test]
    fn test_empty_transport() {
        let transport = MockTransport::new();
        assert_eq!(transport.fixture_count(), 0);
    }
}
