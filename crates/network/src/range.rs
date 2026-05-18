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
//! Multi-range (`bytes=0-99,200-299`) и multipart/byteranges-парсер —
//! см. `RangeRequest::Multi`, `parse_multipart_byteranges`,
//! `MultiRangeResponse`. RFC 7233 §4.1: сервер на multi-range отвечает
//! либо `206` с `Content-Type: multipart/byteranges; boundary=X` (одна
//! `Content-Range` на каждый part), либо `206` с одним `Content-Range`
//! (когда смог объединить пересекающиеся диапазоны), либо `200` с
//! полным телом (Range проигнорирован). Все три исхода нормализованы
//! в `MultiRangeResponse`.

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

/// Запрос range-байт, single- или multi-. `Multi(vec)` сериализуется в
/// `bytes=START1-END1,START2-END2,...` (RFC 7233 §3.1 — comma-separated
/// `ranges-specifier`). Caller получает либо одну `RangeResponse`
/// (через `fetch_range`), либо `MultiRangeResponse` (через
/// `fetch_multi_range`); сервер сам решает, нарезать ответ на parts
/// или объединить в один Content-Range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeRequest {
    /// Один диапазон. Эквивалент прежнего API `fetch_range`.
    Single(RangeSpec),
    /// Несколько диапазонов. Vec обязан содержать хотя бы один валидный
    /// spec, иначе `header_value()` → `None` (header не шлётся, fetch
    /// возвращает full body).
    Multi(Vec<RangeSpec>),
}

impl RangeRequest {
    /// Готовое значение для HTTP-заголовка `Range:` (без префикса).
    /// Возвращает `None` если ни один spec не сериализуется
    /// (`Single(invalid)` или `Multi(пусто/только-invalid)`). Невалидные
    /// spec-ы внутри Multi молча отбрасываются — отправляем то, что
    /// можем (`bytes=0-99, junk, 200-299` → `bytes=0-99,200-299`).
    pub(crate) fn header_value(&self) -> Option<String> {
        match self {
            Self::Single(s) => s.header_value(),
            Self::Multi(specs) => {
                let parts: Vec<String> = specs
                    .iter()
                    .filter_map(|s| s.header_value())
                    .map(|h| h.trim_start_matches("bytes=").to_owned())
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(format!("bytes={}", parts.join(",")))
                }
            }
        }
    }
}

impl From<RangeSpec> for RangeRequest {
    fn from(spec: RangeSpec) -> Self {
        Self::Single(spec)
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

/// Один part в multipart/byteranges-ответе (или единственный part в случае
/// 200/206-single). `content_range = None` — сервер вернул `200 OK` или
/// решил не объявлять Content-Range (нестандартный случай, не валит fetch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangePart {
    pub body: Vec<u8>,
    pub content_range: Option<ContentRange>,
}

/// Ответ на multi-range запрос. Caller получает единый список parts,
/// независимо от того, ответил сервер multipart-ом или объединил всё в
/// один `Content-Range`. `200 OK` (Range проигнорирован) — `parts.len()=1`,
/// `content_range = None`, `body` — полный ресурс.
#[derive(Debug, Clone)]
pub struct MultiRangeResponse {
    pub status: u16,
    pub parts: Vec<RangePart>,
}

/// Извлечь boundary-токен из значения `Content-Type` (RFC 7231 §3.1.1.1 +
/// RFC 2046 §5.1.1). Принимает строку вида
/// `multipart/byteranges; boundary=foo` (case-insensitive type, parameter
/// name; quoted-string значение допускается). Возвращает только сам
/// токен, без кавычек.
///
/// Если type не `multipart/...` или нет parameter `boundary` — `None`.
/// Парсер лояльный: лишние пробелы между токенами игнорируются, parameter
/// перед `boundary` не считается обязательным быть последним.
pub fn parse_boundary_from_content_type(value: &str) -> Option<String> {
    let mut iter = value.split(';');
    let media_type = iter.next()?.trim().to_ascii_lowercase();
    // RFC 7233 §4.1: multipart/byteranges — единственный type для multi-range.
    // Принимаем любой `multipart/*` чтобы не падать на странных серверах,
    // которые присылают `multipart/x-byteranges` или похожее.
    if !media_type.starts_with("multipart/") {
        return None;
    }
    for param in iter {
        let param = param.trim();
        let (name, raw) = param.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("boundary") {
            continue;
        }
        let raw = raw.trim();
        let unquoted = if let Some(s) = raw.strip_prefix('"') {
            s.strip_suffix('"').unwrap_or(s)
        } else {
            raw
        };
        if unquoted.is_empty() {
            return None;
        }
        return Some(unquoted.to_owned());
    }
    None
}

/// Парсер multipart/byteranges body (RFC 7233 §A + RFC 2046 §5.1.1).
///
/// Формат: parts разделены `--<boundary>\r\n`, каждый part — собственный
/// набор headers (включая `Content-Range`, опционально `Content-Type`) +
/// пустая строка + body. После последнего part идёт closing-delimiter
/// `--<boundary>--`.
///
/// Возвращает Vec parts с заполненным `body` (raw bytes, как пришли) и
/// разобранным `Content-Range`. Леиниентый: преамбула / эпилог
/// игнорируется (per spec), отсутствие `Content-Range` в part-е оставляет
/// `content_range=None` (вместо отказа — некоторые серверы шалят), CRLF/LF
/// внутри headers принимаются оба. Возвращает `None` только при полностью
/// невалидном входе (boundary не найден ни разу).
pub fn parse_multipart_byteranges(body: &[u8], boundary: &str) -> Option<Vec<RangePart>> {
    if boundary.is_empty() {
        return None;
    }
    let delim = format!("--{boundary}");
    let delim_bytes = delim.as_bytes();
    let positions = find_all(body, delim_bytes);
    if positions.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(positions.len().saturating_sub(1));
    for window in positions.windows(2) {
        let (start, next) = (window[0], window[1]);
        // Сразу после `--boundary` идут либо `\r\n` (начало part-а), либо
        // `--` (closing delimiter). Closing — конец сообщения, до этого
        // ничего больше парсить не нужно.
        let after_delim = start + delim_bytes.len();
        if after_delim + 2 <= body.len() && &body[after_delim..after_delim + 2] == b"--" {
            // Закрывающий разделитель — части перед ним мы уже добавили
            // в предыдущих итерациях.
            break;
        }
        let part_start = skip_line_terminator(body, after_delim);
        // Часть тянется от part_start до позиции непосредственно перед
        // следующим `--boundary`. Между ними нужно срезать trailing CRLF,
        // который по spec принадлежит boundary-delimiter-у, а не part body.
        let part_end = trim_trailing_line_terminator(body, part_start, next);
        if let Some(part) = parse_one_part(&body[part_start..part_end]) {
            parts.push(part);
        }
    }
    // Если в положениях boundary был только опening-delimiter без
    // closing — допускаем (некоторые «недо-multipart»-ответы). Если
    // closing встретился без opening — невозможно (positions.windows(2)
    // не сработает).
    Some(parts)
}

fn parse_one_part(part: &[u8]) -> Option<RangePart> {
    // Headers до пустой строки (CRLF CRLF или LF LF).
    let hdr_end = find_header_terminator(part)?;
    let header_block = &part[..hdr_end];
    let body_start = hdr_end + header_terminator_len(part, hdr_end);
    let body = part.get(body_start..)?.to_vec();
    let mut content_range: Option<ContentRange> = None;
    for raw_line in split_lines(header_block) {
        let line = std::str::from_utf8(raw_line).ok()?;
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-range")
            && content_range.is_none()
        {
            content_range = parse_content_range(value.trim());
        }
    }
    Some(RangePart { body, content_range })
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || haystack.len() < needle.len() {
        return out;
    }
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            out.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

fn skip_line_terminator(buf: &[u8], pos: usize) -> usize {
    let bytes = buf.get(pos..).unwrap_or(&[]);
    if bytes.starts_with(b"\r\n") {
        pos + 2
    } else if bytes.starts_with(b"\n") {
        pos + 1
    } else {
        pos
    }
}

fn trim_trailing_line_terminator(buf: &[u8], start: usize, end: usize) -> usize {
    if end >= start + 2 && &buf[end - 2..end] == b"\r\n" {
        end - 2
    } else if end > start && buf[end - 1] == b'\n' {
        end - 1
    } else {
        end
    }
}

fn find_header_terminator(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i..].starts_with(b"\r\n\r\n") {
            return Some(i);
        }
        if buf[i..].starts_with(b"\n\n") {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn header_terminator_len(buf: &[u8], pos: usize) -> usize {
    if buf[pos..].starts_with(b"\r\n\r\n") {
        4
    } else if buf[pos..].starts_with(b"\n\n") {
        2
    } else {
        0
    }
}

fn split_lines(buf: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < buf.len() {
        if buf[i..].starts_with(b"\r\n") {
            out.push(&buf[start..i]);
            i += 2;
            start = i;
        } else if buf[i] == b'\n' {
            out.push(&buf[start..i]);
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < buf.len() {
        out.push(&buf[start..]);
    }
    out
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

    // ── RangeRequest encoding ────────────────────────────────────────────

    #[test]
    fn range_request_single_passthrough() {
        let r = RangeRequest::Single(RangeSpec::closed(0, 99));
        assert_eq!(r.header_value().as_deref(), Some("bytes=0-99"));
    }

    #[test]
    fn range_request_single_invalid_is_none() {
        let r = RangeRequest::Single(RangeSpec::closed(100, 50));
        assert!(r.header_value().is_none());
    }

    #[test]
    fn range_request_multi_two_closed() {
        let r = RangeRequest::Multi(vec![
            RangeSpec::closed(0, 99),
            RangeSpec::closed(200, 299),
        ]);
        assert_eq!(r.header_value().as_deref(), Some("bytes=0-99,200-299"));
    }

    #[test]
    fn range_request_multi_mixed_forms() {
        // Closed + open-ended + suffix — все в одной строке.
        let r = RangeRequest::Multi(vec![
            RangeSpec::closed(0, 99),
            RangeSpec::from(1000),
            RangeSpec::suffix(50),
        ]);
        assert_eq!(
            r.header_value().as_deref(),
            Some("bytes=0-99,1000-,-50")
        );
    }

    #[test]
    fn range_request_multi_drops_invalid() {
        // closed(end<start) и suffix(0) — невалидны, пропускаются.
        let r = RangeRequest::Multi(vec![
            RangeSpec::closed(0, 99),
            RangeSpec::closed(100, 50),
            RangeSpec::suffix(0),
            RangeSpec::from(500),
        ]);
        assert_eq!(r.header_value().as_deref(), Some("bytes=0-99,500-"));
    }

    #[test]
    fn range_request_multi_empty_is_none() {
        let r = RangeRequest::Multi(Vec::new());
        assert!(r.header_value().is_none());
    }

    #[test]
    fn range_request_multi_only_invalid_is_none() {
        let r = RangeRequest::Multi(vec![
            RangeSpec::closed(100, 50),
            RangeSpec::suffix(0),
        ]);
        assert!(r.header_value().is_none());
    }

    #[test]
    fn range_request_from_spec_into() {
        // From-impl покрывает удобный paths типа `range.into()`.
        let r: RangeRequest = RangeSpec::closed(0, 9).into();
        assert!(matches!(r, RangeRequest::Single(_)));
    }

    // ── Content-Type boundary parsing ────────────────────────────────────

    #[test]
    fn boundary_basic() {
        assert_eq!(
            parse_boundary_from_content_type("multipart/byteranges; boundary=THIS_STRING_SEPARATES"),
            Some("THIS_STRING_SEPARATES".to_owned())
        );
    }

    #[test]
    fn boundary_quoted() {
        assert_eq!(
            parse_boundary_from_content_type(r#"multipart/byteranges; boundary="abc 123""#),
            Some("abc 123".to_owned())
        );
    }

    #[test]
    fn boundary_case_insensitive_type_and_param() {
        assert_eq!(
            parse_boundary_from_content_type("Multipart/ByteRanges; Boundary=XyZ"),
            Some("XyZ".to_owned())
        );
    }

    #[test]
    fn boundary_extra_whitespace() {
        assert_eq!(
            parse_boundary_from_content_type("multipart/byteranges  ;   boundary=foo  "),
            Some("foo".to_owned())
        );
    }

    #[test]
    fn boundary_with_charset_before_boundary() {
        // Несколько параметров — boundary может идти не первым.
        assert_eq!(
            parse_boundary_from_content_type(
                "multipart/byteranges; charset=utf-8; boundary=foo"
            ),
            Some("foo".to_owned())
        );
    }

    #[test]
    fn boundary_non_multipart_rejected() {
        assert!(parse_boundary_from_content_type("text/html; charset=utf-8").is_none());
        assert!(parse_boundary_from_content_type("application/json").is_none());
    }

    #[test]
    fn boundary_missing_param_rejected() {
        assert!(parse_boundary_from_content_type("multipart/byteranges").is_none());
        assert!(parse_boundary_from_content_type("multipart/byteranges; charset=utf-8").is_none());
    }

    #[test]
    fn boundary_empty_value_rejected() {
        assert!(parse_boundary_from_content_type("multipart/byteranges; boundary=").is_none());
        assert!(parse_boundary_from_content_type(r#"multipart/byteranges; boundary="""#).is_none());
    }

    // ── multipart/byteranges body parser ─────────────────────────────────

    fn build_body(boundary: &str, parts: &[(&str, &[u8])]) -> Vec<u8> {
        // (Content-Range value, body bytes) per part.
        let mut out = Vec::new();
        for (cr, body) in parts {
            out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            out.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
            out.extend_from_slice(format!("Content-Range: {cr}\r\n\r\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        out
    }

    #[test]
    fn multipart_two_parts_basic() {
        let body = build_body(
            "BNDRY",
            &[
                ("bytes 0-4/100", b"hello"),
                ("bytes 10-14/100", b"world"),
            ],
        );
        let parts = parse_multipart_byteranges(&body, "BNDRY").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].body, b"hello");
        assert_eq!(parts[0].content_range, Some(ContentRange { start: 0, end: 4, total: Some(100) }));
        assert_eq!(parts[1].body, b"world");
        assert_eq!(parts[1].content_range, Some(ContentRange { start: 10, end: 14, total: Some(100) }));
    }

    #[test]
    fn multipart_with_preamble_and_epilogue() {
        // Преамбула и эпилог per RFC 2046 §5.1.1 игнорируются.
        let mut body = Vec::new();
        body.extend_from_slice(b"Some preamble text that clients must ignore.\r\n");
        body.extend_from_slice(&build_body("XYZ", &[("bytes 0-2/10", b"abc")]));
        body.extend_from_slice(b"And some trailing epilogue.\r\n");
        let parts = parse_multipart_byteranges(&body, "XYZ").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].body, b"abc");
    }

    #[test]
    fn multipart_binary_body_with_embedded_crlf() {
        // Body содержит \r\n внутри — boundary всё равно находится как
        // уникальный токен (это и есть смысл boundary).
        let body = build_body(
            "BIN",
            &[
                ("bytes 0-7/100", b"a\r\nb\r\nc\r\n"),
                ("bytes 50-51/100", &[0xFF, 0x00]),
            ],
        );
        let parts = parse_multipart_byteranges(&body, "BIN").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].body, b"a\r\nb\r\nc\r\n");
        assert_eq!(parts[1].body, &[0xFF, 0x00]);
    }

    #[test]
    fn multipart_lf_only_line_endings() {
        // Некоторые серверы шалят и шлют \n вместо \r\n. Лояльный парсер
        // обрабатывает оба, чтобы не валить fetch на нестандартных HTTP.
        let mut body = Vec::new();
        body.extend_from_slice(b"--BND\nContent-Range: bytes 0-2/10\n\nabc\n");
        body.extend_from_slice(b"--BND--\n");
        let parts = parse_multipart_byteranges(&body, "BND").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].body, b"abc");
        assert_eq!(parts[0].content_range, Some(ContentRange { start: 0, end: 2, total: Some(10) }));
    }

    #[test]
    fn multipart_missing_content_range_in_part() {
        // RFC 7233 §A требует Content-Range в каждом part, но не валим
        // fetch если сервер забыл — content_range остаётся None.
        let mut body = Vec::new();
        body.extend_from_slice(b"--B\r\nContent-Type: application/octet-stream\r\n\r\ndata\r\n");
        body.extend_from_slice(b"--B--\r\n");
        let parts = parse_multipart_byteranges(&body, "B").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].body, b"data");
        assert!(parts[0].content_range.is_none());
    }

    #[test]
    fn multipart_no_boundary_match_returns_none() {
        assert!(parse_multipart_byteranges(b"this is not multipart", "X").is_none());
    }

    #[test]
    fn multipart_empty_boundary_returns_none() {
        // Пустой boundary запрещён и Content-Type-параметром, и здесь.
        assert!(parse_multipart_byteranges(b"some body", "").is_none());
    }

    #[test]
    fn multipart_only_closing_delimiter_yields_empty_parts() {
        // Сервер вернул только closing — никаких частей. Это не ошибка
        // парсера, просто пустой ответ (никаких byte ranges не пришло).
        let body = b"--B--\r\n";
        let parts = parse_multipart_byteranges(body, "B").unwrap();
        assert!(parts.is_empty());
    }

    #[test]
    fn multipart_part_with_extra_headers_keeps_correct_body() {
        // Множественные headers в part-е — Content-Range берётся первый
        // найденный, остальные игнорируются (Content-Type / Date / ...).
        let mut body = Vec::new();
        body.extend_from_slice(
            b"--Y\r\nDate: Wed, 18 May 2026 00:00:00 GMT\r\nContent-Type: text/plain\r\nContent-Range: bytes 5-7/20\r\n\r\nfoo\r\n",
        );
        body.extend_from_slice(b"--Y--\r\n");
        let parts = parse_multipart_byteranges(&body, "Y").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].body, b"foo");
        assert_eq!(parts[0].content_range, Some(ContentRange { start: 5, end: 7, total: Some(20) }));
    }
}
