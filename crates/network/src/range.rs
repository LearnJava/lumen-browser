//! HTTP Range requests (RFC 7233).
//!
//! Поддержка single-range запросов в трёх формах: закрытая
//! `bytes=START-END`, открытая `bytes=START-` (от START до конца), и
//! suffix `bytes=-N` (последние N байт). Сервер может ответить либо
//! `206 Partial Content` (с заголовком `Content-Range: bytes START-END/TOTAL`),
//! либо `200 OK` с полным телом (Range проигнорирован — RFC 7233 §3.1
//! разрешает оба ответа). Клиент принимает любой исход; `RangeResponse.content_range`
//! показывает, сработало ли частичное чтение.
//!
//! Опциональный `If-Range` (RFC 7233 §3.2) — conditional range, защищает
//! от race condition при resume: если ресурс не изменился (ETag /
//! Last-Modified совпадает), server отдаёт `206` с запрошенным диапазоном;
//! если изменился — `200` с полным новым телом (валидатор стерильно
//! инвалидирует диапазон). Caller передаёт validator из предыдущего
//! ответа (заголовок `ETag` или `Last-Modified`); spec разрешает либо
//! strong ETag, либо HTTP-date.
//!
//! Phase 0 не поддерживает: multi-range (`bytes=0-99,200-299` →
//! multipart/byteranges — отдельный пайплайн парсинга боундари).

/// Спецификация запрашиваемого диапазона байт (inclusive по обоим концам
/// в Closed-варианте). Три формы по RFC 7233 §2.1 «byte-range-spec» +
/// «suffix-byte-range-spec».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSpec {
    /// `bytes=START-END` — закрытый диапазон inclusive по обоим концам.
    /// Если `end < start` — невалидно (см. `header_value`).
    Closed { start: u64, end: u64 },
    /// `bytes=START-` — от `start` до конца ресурса. Сервер обязан
    /// отдать минимум `start..=last_byte_of_resource`. Если
    /// `start >= total_size` — `416 Range Not Satisfiable`.
    OpenEnded { start: u64 },
    /// `bytes=-N` — последние N байт ресурса. RFC 7233 §2.1:
    /// suffix-length > 0 (`bytes=-0` — protocol error). Если N больше
    /// размера ресурса, сервер вправе либо отдать весь ресурс, либо `416`
    /// (поведение implementation-defined).
    Suffix { length: u64 },
}

impl RangeSpec {
    /// Закрытый диапазон `[start; end]` inclusive по обоим концам.
    pub fn closed(start: u64, end: u64) -> Self {
        Self::Closed { start, end }
    }

    /// Открытый диапазон от `start` до конца ресурса.
    pub fn from(start: u64) -> Self {
        Self::OpenEnded { start }
    }

    /// Suffix-range: последние `length` байт ресурса. RFC 7233 §2.1.
    /// `length = 0` сделает невалидный header (`header_value` вернёт None) —
    /// `bytes=-0` не имеет смысла (отдай мне 0 последних байт = ничего).
    pub fn suffix(length: u64) -> Self {
        Self::Suffix { length }
    }

    /// Значение для HTTP-заголовка `Range:` (без префикса `Range: `).
    /// Возвращает `None` если спецификация некорректна: `end < start`
    /// в Closed, `length = 0` в Suffix.
    pub(crate) fn header_value(&self) -> Option<String> {
        match self {
            Self::Closed { start, end } if end < start => None,
            Self::Closed { start, end } => Some(format!("bytes={start}-{end}")),
            Self::OpenEnded { start } => Some(format!("bytes={start}-")),
            Self::Suffix { length: 0 } => None,
            Self::Suffix { length } => Some(format!("bytes=-{length}")),
        }
    }
}

/// Validator для `If-Range` header (RFC 7233 §3.2). Либо ETag (`"abc"`,
/// `W/"weak"`), либо HTTP-date (`Tue, 15 Nov 1994 12:45:26 GMT`). Spec
/// требует **strong** validator (strong ETag или date с точностью до
/// секунды) — иначе race-condition: ресурс мог измениться внутри секунды.
/// Caller-сторона ответственна за выбор подходящего validator-а из
/// предыдущего ответа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeValidator {
    /// ETag (вместе с кавычками и опциональным `W/` префиксом). Передаётся
    /// «как есть» из header-а предыдущего ответа.
    ETag(String),
    /// HTTP-date. Передаётся «как есть» из header-а предыдущего ответа.
    LastModified(String),
}

impl RangeValidator {
    /// Сырое значение для `If-Range:` header-а (без префикса `If-Range: `).
    /// Spec §3.2 не требует никакой трансформации — клиент дословно копирует
    /// validator из предыдущего ответа.
    pub(crate) fn header_value(&self) -> &str {
        match self {
            Self::ETag(t) | Self::LastModified(t) => t,
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
    fn range_spec_suffix_header() {
        let r = RangeSpec::suffix(500);
        assert_eq!(r.header_value().as_deref(), Some("bytes=-500"));
    }

    #[test]
    fn range_spec_suffix_single_byte() {
        let r = RangeSpec::suffix(1);
        assert_eq!(r.header_value().as_deref(), Some("bytes=-1"));
    }

    #[test]
    fn range_spec_suffix_zero_is_none() {
        // bytes=-0 — protocol error по RFC 7233 §2.1 «suffix-length > 0».
        let r = RangeSpec::suffix(0);
        assert!(r.header_value().is_none());
    }

    #[test]
    fn range_validator_etag_strong() {
        let v = RangeValidator::ETag("\"abc123\"".to_owned());
        assert_eq!(v.header_value(), "\"abc123\"");
    }

    #[test]
    fn range_validator_etag_weak() {
        // Weak ETag (`W/"..."`) сохраняется дословно — server решает
        // считать ли его strong-enough для conditional Range (§3.2:
        // server SHOULD use strong validator, MAY accept weak).
        let v = RangeValidator::ETag("W/\"weak-etag\"".to_owned());
        assert_eq!(v.header_value(), "W/\"weak-etag\"");
    }

    #[test]
    fn range_validator_last_modified_date() {
        let v = RangeValidator::LastModified("Tue, 15 Nov 1994 12:45:26 GMT".to_owned());
        assert_eq!(v.header_value(), "Tue, 15 Nov 1994 12:45:26 GMT");
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
