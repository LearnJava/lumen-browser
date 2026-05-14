//! HTTP Range requests (RFC 7233).
//!
//! Поддержка single-range запросов через `Range: bytes=START-END[или START-]`.
//! Сервер может ответить либо `206 Partial Content` (с заголовком
//! `Content-Range: bytes START-END/TOTAL`), либо `200 OK` с полным телом
//! (Range проигнорирован — RFC 7233 §3.1 разрешает оба ответа). Клиент
//! принимает любой исход; `RangeResponse.content_range` показывает,
//! сработало ли частичное чтение.
//!
//! Phase 0 не поддерживает: suffix-range (`bytes=-N`), multi-range
//! (`bytes=0-99,200-299` → multipart/byteranges — отдельный пайплайн
//! парсинга боундари), If-Range conditional (нужен для resume без гонки
//! с изменением ресурса). Все добавятся при необходимости.

/// Спецификация запрашиваемого диапазона байт (inclusive).
///
/// `end = None` означает «от `start` до конца ресурса». Сервер обязан
/// отдать минимум `start..=last_byte_of_resource`. Если `start >=
/// total_size` — сервер ответит `416 Range Not Satisfiable`, и `fetch_range`
/// вернёт `Err`. `end < start` — protocol error, отрезаем в `header_value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeSpec {
    pub start: u64,
    pub end: Option<u64>,
}

impl RangeSpec {
    /// Закрытый диапазон `[start; end]` inclusive по обоим концам.
    pub fn closed(start: u64, end: u64) -> Self {
        Self {
            start,
            end: Some(end),
        }
    }

    /// Открытый диапазон от `start` до конца ресурса.
    pub fn from(start: u64) -> Self {
        Self { start, end: None }
    }

    /// Значение для HTTP-заголовка `Range:` (без префикса `Range: `).
    /// Возвращает `None` если `end < start` (некорректная спецификация).
    pub(crate) fn header_value(&self) -> Option<String> {
        match self.end {
            Some(end) if end < self.start => None,
            Some(end) => Some(format!("bytes={}-{}", self.start, end)),
            None => Some(format!("bytes={}-", self.start)),
        }
    }
}

/// Разобранный `Content-Range: bytes START-END/TOTAL` (RFC 7233 §4.2).
///
/// `total = None` соответствует `*` в позиции total — сервер знает range,
/// но не знает (или не хочет раскрывать) общий размер ресурса. Поле `end`
/// inclusive — последний возвращённый байт, не «один за концом».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: Option<u64>,
}

/// Парсер `Content-Range: bytes START-END/TOTAL`. Поддерживает обе формы
/// total (`/N` или `/*`). Любой иной формат (`bytes */N` — для 416,
/// `items 0-9/*` — non-bytes unit) даёт `None`. Парсер строгий: лишние
/// пробелы между числами не допускаются (`bytes 0 - 99/100` — invalid).
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let s = value.trim();
    let rest = s.strip_prefix("bytes")?.trim_start();
    let (range_part, total_part) = rest.split_once('/')?;
    let (start_s, end_s) = range_part.trim().split_once('-')?;
    let start = start_s.trim().parse::<u64>().ok()?;
    let end = end_s.trim().parse::<u64>().ok()?;
    if end < start {
        return None;
    }
    let total = match total_part.trim() {
        "*" => None,
        n => Some(n.parse::<u64>().ok()?),
    };
    Some(ContentRange { start, end, total })
}

/// Ответ на range-запрос. `status = 206` — Range honored (Content-Range
/// заполнен); `status = 200` — сервер вернул full body, `content_range`
/// будет `None`. Любой другой код доходит как `Err` из `fetch_range`.
#[derive(Debug, Clone)]
pub struct RangeResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_range: Option<ContentRange>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_spec_closed_header() {
        let r = RangeSpec::closed(0, 499);
        assert_eq!(r.header_value().as_deref(), Some("bytes=0-499"));
    }

    #[test]
    fn range_spec_open_header() {
        let r = RangeSpec::from(500);
        assert_eq!(r.header_value().as_deref(), Some("bytes=500-"));
    }

    #[test]
    fn range_spec_closed_single_byte() {
        // bytes=5-5 — один байт, валидно.
        let r = RangeSpec::closed(5, 5);
        assert_eq!(r.header_value().as_deref(), Some("bytes=5-5"));
    }

    #[test]
    fn range_spec_invalid_end_lt_start_is_none() {
        let r = RangeSpec::closed(100, 50);
        assert!(r.header_value().is_none());
    }

    #[test]
    fn content_range_basic() {
        let cr = parse_content_range("bytes 0-499/1234").unwrap();
        assert_eq!(cr, ContentRange { start: 0, end: 499, total: Some(1234) });
    }

    #[test]
    fn content_range_with_extra_whitespace() {
        let cr = parse_content_range("  bytes 0-499/1234  ").unwrap();
        assert_eq!(cr.start, 0);
        assert_eq!(cr.end, 499);
        assert_eq!(cr.total, Some(1234));
    }

    #[test]
    fn content_range_unknown_total() {
        let cr = parse_content_range("bytes 1000-1999/*").unwrap();
        assert_eq!(cr, ContentRange { start: 1000, end: 1999, total: None });
    }

    #[test]
    fn content_range_full_resource() {
        let cr = parse_content_range("bytes 0-9/10").unwrap();
        assert_eq!(cr.start, 0);
        assert_eq!(cr.end, 9);
        assert_eq!(cr.total, Some(10));
    }

    #[test]
    fn content_range_non_bytes_unit_rejected() {
        // RFC 7233 §4.2 — unit может быть other-range-unit, но Phase 0
        // понимает только `bytes`.
        assert!(parse_content_range("items 0-9/100").is_none());
    }

    #[test]
    fn content_range_missing_slash_rejected() {
        assert!(parse_content_range("bytes 0-99").is_none());
    }

    #[test]
    fn content_range_missing_dash_rejected() {
        assert!(parse_content_range("bytes 099/100").is_none());
    }

    #[test]
    fn content_range_end_lt_start_rejected() {
        assert!(parse_content_range("bytes 99-50/100").is_none());
    }

    #[test]
    fn content_range_non_numeric_rejected() {
        assert!(parse_content_range("bytes a-b/100").is_none());
        assert!(parse_content_range("bytes 0-99/foo").is_none());
    }

    #[test]
    fn content_range_unsatisfied_response_form_rejected() {
        // RFC 7233 §4.4 — `bytes */N` отправляется в 416 Range Not Satisfiable
        // (вместо START-END). Phase 0 трактует как невалидную для 206 контекста.
        // 416 fetch_range всё равно вернёт Err по status code.
        assert!(parse_content_range("bytes */1234").is_none());
    }

    #[test]
    fn content_range_empty_rejected() {
        assert!(parse_content_range("").is_none());
    }
}
