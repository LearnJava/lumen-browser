//! HTTP/1.1 request-line + header serialization ([`Connection::write_request`]).
//! Split out of `network/lib.rs` (SPLIT-NW0).

use std::io::Write;

use lumen_core::error::{Error, Result};

use crate::{Connection, HttpProfile, RangeRequest, RangeValidator, RequestBody, apply_ua_override, http};

impl Connection {
    /// Записать HTTP-запрос в stream. Используется `Connection: keep-alive`
    /// (HTTP/1.1 default, но явно для ясности и для совместимости с серверами,
    /// которые криво интерпретируют отсутствие хедера). Опциональный `range`
    /// добавляет header `Range: bytes=START-END` / `bytes=START-` / `bytes=-N`
    /// (RFC 7233 §3.1); невалидный RangeSpec (`end < start`, `suffix=0`)
    /// тихо опускает header — fetch получит full response (200 OK), не упадёт.
    /// Опциональный `if_range` — `If-Range` validator (RFC 7233 §3.2),
    /// добавляется только вместе с Range. Опциональный `authorization` —
    /// готовая строка для header `Authorization` (Basic / Digest),
    /// формируется на уровень выше после 401-retry.
    /// Опциональный `body` — тело запроса (POST/PUT/PATCH/DELETE); добавляет
    /// `Content-Type` и `Content-Length` и дописывает байты после заголовков.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_request(
        &mut self,
        method: &str,
        host: &str,
        path: &str,
        range: Option<&RangeRequest>,
        if_range: Option<&RangeValidator>,
        authorization: Option<&str>,
        accept_encoding: Option<&str>,
        extra_headers: &str,
        http_profile: HttpProfile,
        body: Option<&RequestBody<'_>>,
    ) -> Result<()> {
        let range_value = range.and_then(|r| r.header_value());
        let range_header = match &range_value {
            Some(value) => format!("Range: {value}\r\n"),
            None => String::new(),
        };
        // If-Range шлём только если есть валидный Range — header без Range
        // ничего не значит для сервера (RFC 7233 §3.2 «sent with a Range
        // header field»).
        let if_range_header = match (&range_value, if_range) {
            (Some(_), Some(v)) => format!("If-Range: {}\r\n", v.header_value()),
            _ => String::new(),
        };
        let auth_header = match authorization {
            Some(value) => format!("Authorization: {value}\r\n"),
            None => String::new(),
        };
        // `extra_headers` уже содержит свои CRLF после каждой строки (формат
        // pre-built). Используется CORS-путём для `Origin` / `Access-Control-*`
        // и для пользовательских author-headers. Caller гарантирует, что
        // среди них нет дублей `Host`/`Connection`/`Content-Length` и т.п.
        //
        // Content-Type/Content-Length тела — там же, где остальные не-fingerprint
        // заголовки (Chrome order): после Range/If-Range/Auth, перед extra.
        let body_headers = match body {
            Some(b) => format!(
                "Content-Type: {}\r\nContent-Length: {}\r\n",
                b.content_type,
                b.bytes.len()
            ),
            None => String::new(),
        };
        // Range/If-Range/Auth идут после fingerprint-заголовков (Chrome order).
        let combined_extra =
            format!("{range_header}{if_range_header}{auth_header}{body_headers}{extra_headers}");
        let accept_enc = accept_encoding.unwrap_or("");
        let header_block = http::build_request_headers(host, accept_enc, &combined_extra, http_profile);
        let header_block = apply_ua_override(header_block);
        let req = format!("{method} {path} HTTP/1.1\r\n{header_block}");
        let stream = self.reader.get_mut();
        stream
            .write_all(req.as_bytes())
            .map_err(|e| Error::Network(format!("write request: {e}")))?;
        if let Some(b) = body {
            stream
                .write_all(b.bytes)
                .map_err(|e| Error::Network(format!("write body: {e}")))?;
        }
        stream
            .flush()
            .map_err(|e| Error::Network(format!("flush request: {e}")))?;
        Ok(())
    }
}
