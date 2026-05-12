//! Lumen URL — пока что тонкая обёртка над String с минимальной валидацией.
//!
//! Намеренно не используем `url` crate напрямую: оборачивая, мы можем заменить
//! реализацию (например, на WHATWG-совместимый парсер с поддержкой IDN) без
//! правки всех потребителей. Это пример swap-point из §11 плана.

use crate::error::{Error, Result};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Url(String);

impl Url {
    pub fn parse(s: &str) -> Result<Self> {
        if s.is_empty() {
            return Err(Error::InvalidUrl("empty URL".into()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        assert!(Url::parse("https://wikipedia.org").is_ok());
    }

    #[test]
    fn parse_empty_fails() {
        assert!(Url::parse("").is_err());
    }

    #[test]
    fn parse_cyrillic_domain() {
        // IDN-домен на этапе Phase 0 принимаем «как есть»; правильная
        // Punycode-конвертация — задача §10.3, реализуется в network-слое.
        let u = Url::parse("https://президент.рф/").unwrap();
        assert_eq!(u.as_str(), "https://президент.рф/");
    }
}
