//! Категории ошибок, общие для всего проекта. Каждый модуль может оборачивать
//! свои частные ошибки в подходящий вариант через `From`.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Network(String),
    Parse(String),
    Io(String),
    Storage(String),
    InvalidUrl(String),
    PermissionDenied(String),
    NotFound(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(s) => write!(f, "network error: {s}"),
            Self::Parse(s) => write!(f, "parse error: {s}"),
            Self::Io(s) => write!(f, "io error: {s}"),
            Self::Storage(s) => write!(f, "storage error: {s}"),
            Self::InvalidUrl(s) => write!(f, "invalid url: {s}"),
            Self::PermissionDenied(s) => write!(f, "permission denied: {s}"),
            Self::NotFound(s) => write!(f, "not found: {s}"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
