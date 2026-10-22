//! JS-facing network providers of `HttpClient`: `JsFetchProvider`
//! (fetch/XHR, CSP gates, WebTransport), `WebSocketProvider`/`SseProvider`
//! and the JS WebSocket/EventSource session bridges. Split out of
//! `network/lib.rs` (file-size gate, docs/lint-policy.md §5.1).

use super::*;

fn http_status_text(status: u16) -> &'static str {
    match status {
        100 => "Continue", 101 => "Switching Protocols", 200 => "OK",
        201 => "Created", 202 => "Accepted", 204 => "No Content",
        206 => "Partial Content", 301 => "Moved Permanently", 302 => "Found",
        303 => "See Other", 304 => "Not Modified", 307 => "Temporary Redirect",
        308 => "Permanent Redirect", 400 => "Bad Request", 401 => "Unauthorized",
        403 => "Forbidden", 404 => "Not Found", 405 => "Method Not Allowed",
        408 => "Request Timeout", 409 => "Conflict", 410 => "Gone",
        413 => "Content Too Large", 414 => "URI Too Long",
        422 => "Unprocessable Content", 429 => "Too Many Requests",
        500 => "Internal Server Error", 501 => "Not Implemented",
        502 => "Bad Gateway", 503 => "Service Unavailable",
        504 => "Gateway Timeout", _ => "",
    }
}

/// Effective directive name `CspPolicy::preload_fetch_allows` judges a
/// preload of destination `as_kind` by (for the violation report).
fn csp_directive_name_for_preload(as_kind: &str) -> &'static str {
    match as_kind {
        "script" => "script-src-elem",
        "style" => "style-src-elem",
        "image" | "icon" => "img-src",
        "font" => "font-src",
        "audio" | "video" | "track" => "media-src",
        "fetch" | "json" => "connect-src",
        "worker" | "sharedworker" => "worker-src",
        _ => "prefetch-src",
    }
}

impl JsFetchProvider for HttpClient {
    /// Единственный настоящий путь JS-запроса: и GET, и запрос с телом, и
    /// author-заголовки страницы (BUG-749) — четыре метода ниже сводятся сюда.
    ///
    /// Заголовки уезжают через слот `extra_request_headers` рядом с `Cookie`,
    /// CORS-`Origin` и cache-валидаторами, так что author-значение вытесняет
    /// одноимённый заголовок нашего fingerprint-набора (`Accept`,
    /// `Accept-Language`, `Cache-Control`, `User-Agent`), а не дублирует его.
    fn fetch_request(&self, req: &JsFetchRequest<'_>) -> Result<JsFetchResult> {
        self.fetch_request_impl(req, true)
    }

    /// Сеть самого service worker-а: тот же путь, но перехватчик пропускается,
    /// иначе воркер отправил бы запрос самому себе и встал бы на ожидании
    /// собственного ответа (см. описание метода в `lumen_core::ext`).
    fn fetch_bypassing_sw(&self, url: &str, method: &str) -> Result<JsFetchResult> {
        self.fetch_request_impl(
            &JsFetchRequest {
                url,
                method,
                headers: &[],
                body: None,
                mode: "",
                destination: "",
                token: None,
            },
            false,
        )
    }

    fn fetch_sync(&self, url: &str, method: &str) -> Result<JsFetchResult> {
        self.fetch_request(&JsFetchRequest {
            url,
            method,
            headers: &[],
            body: None,
            mode: "",
            destination: "",
            token: None,
        })
    }

    fn fetch_with_body_sync(
        &self,
        url: &str,
        method: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<JsFetchResult> {
        self.fetch_request(&JsFetchRequest {
            url,
            method,
            headers: &[],
            body: Some(JsFetchBody { content_type, bytes: body }),
            mode: "",
            destination: "",
            token: None,
        })
    }

    /// BUG-1116: `<link rel=preload|modulepreload|prefetch>`'s own fetch,
    /// routed through [`Self::subresource_cache`] when one is attached. A hit
    /// shares bytes with whatever else fetched the same URL this navigation
    /// (real `<script src>`/`<link rel=stylesheet>`, or the engine's own
    /// early streaming warm-up of this same hint — see
    /// `crates/shell/src/page_load.rs::feed_preload_and_emit`); a miss falls
    /// through to the same `fetch_subresource_with_content_type` a real
    /// consumer would have used, so the cache never serves the wrong bytes.
    /// No `subresource_cache` attached → plain uncached GET, same as before
    /// this bug's fix.
    ///
    /// Like every other subresource loader in this engine
    /// (`fetch_subresource_with_content_type`'s callers in
    /// `crates/shell/src/scripts.rs`/`stylesheets.rs`), this does not surface
    /// the HTTP status code — a non-2xx response is still `Ok`, only a
    /// transport-level failure is `Err`. `status`/`ok` are hardcoded to
    /// `200`/`true` on the `Ok` path to match.
    fn fetch_preload_cached(&self, url: &str, as_kind: &str, nonce: &str) -> Result<JsFetchResult> {
        // BUG-1491: a hint the document's CSP forbids is not requested at all
        // — the warm-up (`page_pipeline::warm_preload_cache`) asks the same
        // predicate, so neither path reaches the network.
        self.preload_src_gate(as_kind, url, nonce)?;
        let Some(cache) = self.subresource_cache.clone() else {
            return self.fetch_sync(url, "GET");
        };
        let parsed = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        // The cached bytes may reach a real `<script src>`/`@font-face`
        // consumer, so the request is gated as that consumer's destination —
        // the same one `page_pipeline::warm_preload_cache` uses for a hint
        // the streaming scanner saw first.
        let destination =
            RequestDestination::for_preload_as(as_kind).unwrap_or(RequestDestination::Prefetch);
        let generation = cache.generation();
        let url_owned = url.to_owned();
        let fetch = Box::new(move || {
            self.fetch_subresource_with_content_type(&parsed, destination)
                .map_err(|e| e.to_string())
        });
        match cache.get_or_fetch(generation, &url_owned, fetch) {
            Ok((body, content_type)) => Ok(JsFetchResult {
                status: 200,
                status_text: "OK".to_owned(),
                headers: content_type
                    .into_iter()
                    .map(|ct| ("content-type".to_owned(), ct))
                    .collect(),
                body,
                url: url_owned,
            }),
            Err(e) => Err(Error::Network(e)),
        }
    }

    /// BUG-1116: only a successful warm-up counts — a failed one is not
    /// trusted, the caller retries over the network and reports its own error.
    fn fetch_preloaded(&self, url: &str) -> Option<JsFetchResult> {
        let (body, content_type) = self.subresource_cache.as_ref()?.lookup(url)?.ok()?;
        Some(JsFetchResult {
            status: 200,
            status_text: "OK".to_owned(),
            headers: content_type.into_iter().map(|ct| ("content-type".to_owned(), ct)).collect(),
            body,
            url: url.to_owned(),
        })
    }

    /// Synchronous GET/HEAD fetch that honours an `AbortToken` in-flight.
    ///
    /// Pre-flight: an already-aborted token short-circuits with `Error::Aborted`
    /// before any socket work. Otherwise the token is installed on the current
    /// thread for the duration of the (synchronous) request via [`AbortScope`];
    /// `do_request` deep in the read path spawns an [`AbortWatchdog`] that shuts
    /// the socket down if `abort()` fires mid-response, surfacing as
    /// `Error::Aborted`. The JS layer maps that to a DOMException `AbortError`.
    fn fetch_cancellable(&self, url: &str, method: &str, token: &AbortToken) -> Result<JsFetchResult> {
        self.fetch_request(&JsFetchRequest {
            url,
            method,
            headers: &[],
            body: None,
            mode: "",
            destination: "",
            token: Some(token),
        })
    }

    /// In-flight-cancellable variant of `fetch_with_body_sync`.
    ///
    /// Same teardown machinery as [`fetch_cancellable`](Self::fetch_cancellable):
    /// a pre-flight `is_aborted` short-circuit, then an [`AbortScope`] installs the
    /// token for the request duration so `do_request` deep in the read path spawns
    /// an [`AbortWatchdog`] that shuts the socket down if `abort()` fires mid-response,
    /// surfacing as `Error::Aborted`.
    fn fetch_with_body_cancellable(
        &self,
        url: &str,
        method: &str,
        content_type: &str,
        body: &[u8],
        token: &AbortToken,
    ) -> Result<JsFetchResult> {
        self.fetch_request(&JsFetchRequest {
            url,
            method,
            headers: &[],
            body: Some(JsFetchBody { content_type, bytes: body }),
            mode: "",
            destination: "",
            token: Some(token),
        })
    }

    /// GAP-CSPENF срез 12: `navigator.sendBeacon()`'s pre-spawn `connect-src` check
    /// — same `connect_src_gate` that `fetch_request_impl` runs, called from the
    /// native `_lumen_send_beacon` binding before it hands the request to a
    /// detached background thread (see trait doc comment for why beacon needs its
    /// own hook instead of relying on `fetch_with_body_sync`'s existing gate).
    fn check_connect_src(&self, url: &str) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        let url = self.upgrade_insecure_requests_url(url);
        self.connect_src_gate(&url)
    }

    /// GAP-CSPENF срез 13: `worker-src`/`default-src` pre-check for
    /// `new Worker(url)`/`new SharedWorker(url)`'s classic script fetch,
    /// called by the native `_lumen_worker_fetch_script`/`_lumen_sw_fetch_script`
    /// bindings before they touch `fetch_worker_script` — same "not a single
    /// outgoing byte" shape as [`Self::check_connect_src`], different directive.
    fn check_worker_src(&self, url: &str) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        self.worker_src_gate(&url)
    }

    /// GAP-CSPENF срез 16: `object-src`/`default-src` pre-check for
    /// `<embed src>`/`<object data>` resource fetches, called by the native
    /// `_lumen_check_object_src` binding before the shim's `fetch()` call —
    /// same "not a single outgoing byte" shape as [`Self::check_worker_src`],
    /// different directive.
    fn check_object_src(&self, url: &str) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        self.object_src_gate(&url)
    }

    /// GAP-CSPENF срез 17: `media-src`/`default-src` pre-check for
    /// `<video src>`/`<audio src>`/`<track src>` resource fetches, called by the
    /// native `_lumen_check_media_src` binding before the shim hands the URL to
    /// its loader — same "not a single outgoing byte" shape as
    /// [`Self::check_object_src`], different directive.
    fn check_media_src(&self, url: &str) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        self.media_src_gate(&url)
    }

    /// BUG-1175: `script-src`/`style-src` pre-check for a `<script src>`/
    /// `<link rel=stylesheet>`/`@import` that a script inserted, called by the
    /// native `_lumen_check_element_src` binding before the shim's `fetch()` —
    /// same "not a single outgoing byte" shape as [`Self::check_media_src`].
    /// The URL is upgraded first, as `fetch_request_impl` will send it.
    fn check_element_src(
        &self,
        destination: &str,
        url: &str,
        nonce: &str,
        integrity: &str,
        parser_inserted: bool,
    ) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        let url = self.upgrade_insecure_requests_url(url);
        self.element_src_gate(destination, &url, nonce, integrity, parser_inserted)
    }

    /// BUG-568: the inline `script-src` check for a `<script>`
    /// `document.write()` wrote, against the same policies as
    /// [`Self::check_element_src`].
    fn check_inline_script(&self, nonce: &str, body: &str) -> Result<()> {
        let Some((policies, _, original_policy)) = &self.element_src_policy else {
            return Ok(());
        };
        let nonce = Some(nonce).filter(|n| !n.is_empty());
        if policies
            .iter()
            .all(|policy| policy.inline_allows(&csp::CspDirective::ScriptSrc, nonce, body))
        {
            return Ok(());
        }
        Err(Error::CspElementSrcBlocked {
            directive: "script-src-elem".to_owned(),
            blocked_uri: "inline".to_owned(),
            original_policy: original_policy.clone(),
        })
    }

    /// GAP-CSPENF срез 51: `upgrade-insecure-requests` for `<audio src>` —
    /// the JS shim resolves the URL to absolute (`_abs`) for its
    /// `_lumen_check_media_src` call anyway, so this reuses that same
    /// value rather than re-resolving. An unparseable `url` is returned
    /// unchanged: the caller (`PlatformAudioPlayer::load`) will fail it on
    /// its own parse attempt, same as today.
    fn upgrade_insecure_request_url(&self, url: &str) -> String {
        match Url::parse(url) {
            Ok(parsed) => self.upgrade_insecure_requests_url(parsed).to_string(),
            Err(_) => url.to_string(),
        }
    }

    /// GAP-POLICYREPORT (BUG-953): returns the `sync-xhr` disposition
    /// precomputed by `crate::document_policy` (resolved by the shell in
    /// `page_pipeline::parse_and_layout` and attached via
    /// [`Self::with_sync_xhr_policy`]). `None` means the document carried no
    /// `Document-Policy`/`Document-Policy-Report-Only` header disabling the
    /// feature.
    fn document_policy_sync_xhr_disposition(&self) -> Option<lumen_core::ext::PolicyDisposition> {
        self.sync_xhr_policy.0
    }

    /// Same as [`Self::document_policy_sync_xhr_disposition`], for
    /// `Permissions-Policy`/`Permissions-Policy-Report-Only` — a separate
    /// header, checked independently per spec (GAP-POLICYREPORT, BUG-953).
    fn permissions_policy_sync_xhr_disposition(&self) -> Option<lumen_core::ext::PolicyDisposition> {
        self.sync_xhr_policy.1
    }

    /// GAP-WEBTRANSPORT срез 2b: opens the RFC 9220 Extended CONNECT session
    /// a `new WebTransport(url)` construction needs — a fresh QUIC connect
    /// ([`h3::client_transport::h3_connect`]) followed by the Extended
    /// CONNECT leg ([`h3::client_transport::h3_extended_connect_on_driver`],
    /// срез 2a). Unlike ordinary `fetch()` this does not consult
    /// [`Self::h3_alt_svc`]/`http3_enabled` — WebTransport is QUIC-native by
    /// definition (WHATWG WebTransport §5.1), not an opportunistic upgrade of
    /// an HTTP request.
    ///
    /// The confirmed driver is kept alive in [`Self::webtransport_sessions`]
    /// under a fresh handle rather than dropped: a later slice (uni/bidi
    /// streams, срез 3; datagrams, срез 4) reuses the same live session.
    fn webtransport_connect(&self, url: &str) -> Result<lumen_core::ext::JsWebTransportSession> {
        let parsed = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        if parsed.scheme() != "https" {
            return Err(Error::InvalidUrl(
                "WebTransport requires an https: URL".to_string(),
            ));
        }
        let host = parsed.host();
        if host.is_empty() {
            return Err(Error::InvalidUrl("WebTransport URL has no host".to_string()));
        }
        let port = parsed.port().unwrap_or(443);
        let path = if parsed.path().is_empty() { "/" } else { parsed.path() };

        let config = h3::client_bootstrap::ClientConnectConfig::default();
        let mut driver = h3::client_transport::h3_connect(
            self.resolver.as_ref(),
            host,
            port,
            &config,
            H3_CONNECT_TURNS,
        )
        .map_err(|e| Error::Network(format!("WebTransport connect: {e}")))?;

        let (stream_id, head) = h3::client_transport::h3_extended_connect_on_driver(
            &mut driver,
            host,
            port,
            b"webtransport",
            path.as_bytes(),
            &[],
            H3_REQUEST_TURNS,
        )
        .map_err(|e| Error::Network(format!("WebTransport Extended CONNECT: {e}")))?;

        if !(200..300).contains(&head.status) {
            return Err(Error::Network(format!(
                "WebTransport Extended CONNECT rejected: status {}",
                head.status
            )));
        }

        let handle = {
            let mut next = self.webtransport_next_handle.lock().unwrap_or_else(|e| e.into_inner());
            let id = *next;
            *next += 1;
            id
        };
        self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(
            handle,
            WebTransportSession {
                driver,
                session_id: stream_id,
                peer_initial_max_stream_data_uni: config.initial_max_stream_data_uni,
                next_uni_stream_number: 0,
                peer_initial_max_stream_data_bidi: config.initial_max_stream_data_bidi_remote,
                next_bidi_stream_number: 0,
                pending_peer_uni_headers: std::collections::HashMap::new(),
                peer_uni_stream_leftover: std::collections::HashMap::new(),
                pending_peer_bidi_headers: std::collections::HashMap::new(),
                peer_bidi_stream_leftover: std::collections::HashMap::new(),
                pending_close_capsule: Vec::new(),
            },
        );

        Ok(lumen_core::ext::JsWebTransportSession { handle, status: head.status })
    }

    /// GAP-WEBTRANSPORT срез 3b: opens the first client-initiated
    /// unidirectional QUIC stream for the session `handle` names
    /// ([`h3::client_transport::h3_webtransport_open_uni_stream_on_driver`],
    /// срез 3a) and hands its QUIC stream id back — the actual write-bytes
    /// primitive is a later slice, this one only proves the session handle
    /// reaches a live driver and the stream opens on the wire.
    fn webtransport_open_uni_stream(&self, handle: i32) -> Result<u64> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        let uni_stream_number = session.next_uni_stream_number;
        let stream_id = h3::client_transport::h3_webtransport_open_uni_stream_on_driver(
            &mut session.driver,
            uni_stream_number,
            session.peer_initial_max_stream_data_uni,
            session.session_id,
        )
        .map_err(|e| Error::Network(format!("WebTransport open uni stream: {e}")))?;
        session.next_uni_stream_number += 1;
        Ok(stream_id)
    }

    /// GAP-WEBTRANSPORT срез 3c: writes `data` to the already-open
    /// unidirectional stream `stream_id` on the session `handle` names
    /// ([`h3::client_transport::h3_webtransport_write_stream_on_driver`]) —
    /// the write-bytes primitive `webtransport_open_uni_stream` (срез 3b)
    /// left for a later slice.
    fn webtransport_write_uni_stream(&self, handle: i32, stream_id: u64, data: &[u8]) -> Result<()> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        h3::client_transport::h3_webtransport_write_stream_on_driver(&mut session.driver, stream_id, data)
            .map_err(|e| Error::Network(format!("WebTransport write stream: {e}")))
    }

    /// GAP-WEBTRANSPORT срез 3d: closes the already-open unidirectional
    /// stream `stream_id` on the session `handle` names, sending a QUIC
    /// STREAM FIN ([`h3::client_transport::h3_webtransport_close_uni_stream_on_driver`])
    /// — `WritableStreamDefaultWriter.close()`'s native counterpart.
    fn webtransport_close_uni_stream(&self, handle: i32, stream_id: u64) -> Result<()> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        h3::client_transport::h3_webtransport_close_uni_stream_on_driver(&mut session.driver, stream_id)
            .map_err(|e| Error::Network(format!("WebTransport close stream: {e}")))
    }

    /// GAP-WEBTRANSPORT срез 3d: abruptly terminates the already-open
    /// unidirectional stream `stream_id` on the session `handle` names with
    /// `error_code`, sending a QUIC RESET_STREAM
    /// ([`h3::client_transport::h3_webtransport_reset_uni_stream_on_driver`])
    /// — `WritableStreamDefaultWriter.abort(reason)`'s native counterpart.
    fn webtransport_abort_uni_stream(&self, handle: i32, stream_id: u64, error_code: u64) -> Result<()> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        h3::client_transport::h3_webtransport_reset_uni_stream_on_driver(
            &mut session.driver,
            stream_id,
            error_code,
        )
        .map_err(|e| Error::Network(format!("WebTransport abort stream: {e}")))
    }

    /// GAP-WEBTRANSPORT срез 4b: `createBidirectionalStream()`'s transport
    /// primitive — opens a client-initiated bidirectional QUIC stream on the
    /// session `handle` names
    /// ([`h3::client_transport::h3_webtransport_open_bidi_stream_on_driver`],
    /// срез 4a) and hands its stream id back. The write half is addressed
    /// through the same `webtransport_write_uni_stream`/
    /// `webtransport_close_uni_stream`/`webtransport_abort_uni_stream` the uni
    /// primitive uses — a `SendStream` does not know its own direction. The
    /// read half (data the peer writes back) is `webtransport_read_bidi_stream`
    /// (срез 4c); a unidirectional stream carries no read half by
    /// definition (RFC 9000 §2.1), so it needs no counterpart.
    fn webtransport_open_bidi_stream(&self, handle: i32) -> Result<u64> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        let bidi_stream_number = session.next_bidi_stream_number;
        let stream_id = h3::client_transport::h3_webtransport_open_bidi_stream_on_driver(
            &mut session.driver,
            bidi_stream_number,
            session.peer_initial_max_stream_data_bidi,
            session.session_id,
        )
        .map_err(|e| Error::Network(format!("WebTransport open bidi stream: {e}")))?;
        session.next_bidi_stream_number += 1;
        Ok(stream_id)
    }

    /// GAP-WEBTRANSPORT срез 4c: drains whatever bytes have arrived so far
    /// on the bidi stream `stream_id` (`webtransport_open_bidi_stream`'s
    /// return value) — `WebTransportBidirectionalStream.readable`'s
    /// non-blocking poll primitive. Drives one non-blocking sweep of the
    /// session's transport ([`h3::client_transport::h3_webtransport_read_stream_on_driver`])
    /// before reading, since nothing else keeps this driver's socket drained
    /// once the session is established (unlike an ordinary request, which
    /// `RequestDriver::run` drives to completion).
    fn webtransport_read_bidi_stream(&self, handle: i32, stream_id: u64) -> Result<(Vec<u8>, bool)> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        let bytes = h3::client_transport::h3_webtransport_read_stream_on_driver(&mut session.driver, stream_id)
            .map_err(|e| Error::Network(format!("WebTransport read stream: {e}")))?;
        let finished =
            h3::client_transport::h3_webtransport_stream_finished_on_driver(&session.driver, stream_id);
        Ok((bytes, finished))
    }

    /// GAP-WEBTRANSPORT срез 4d: `incomingUnidirectionalStreams`'s discovery
    /// primitive — drains one non-blocking sweep of the session `handle`
    /// names' transport, finds every peer-initiated unidirectional stream
    /// newly opened toward us since the last call
    /// ([`h3::client_transport::h3_webtransport_poll_new_peer_streams_on_driver`],
    /// filtered to [`h3::stream::is_unidirectional`] ids — a discovered
    /// bidirectional id is classified too, by [`Self::classify_discovered_peer_streams`],
    /// but only [`Self::webtransport_poll_incoming_bidi_streams`] reports it),
    /// and returns the ids whose WebTransport stream header has now fully
    /// arrived and been stripped
    /// ([`h3::client_transport::parse_webtransport_uni_header`]) — ready for
    /// [`Self::webtransport_read_incoming_uni_stream`].
    ///
    /// An id discovered this call but whose header is still incomplete (split
    /// across more than one STREAM frame) is not returned yet; it stays
    /// queued in [`WebTransportSession::pending_peer_uni_headers`] and is
    /// retried on the next call, same as every other id still pending.
    fn webtransport_poll_incoming_uni_streams(&self, handle: i32) -> Result<Vec<u64>> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;

        Self::classify_discovered_peer_streams(session)?;

        let mut ready = Vec::new();
        let pending_ids: Vec<u64> = session.pending_peer_uni_headers.keys().copied().collect();
        for id in pending_ids {
            let chunk =
                h3::client_transport::h3_webtransport_read_stream_on_driver(&mut session.driver, id)
                    .map_err(|e| Error::Network(format!("WebTransport poll incoming streams: {e}")))?;
            let buf = session.pending_peer_uni_headers.entry(id).or_default();
            buf.extend_from_slice(&chunk);
            if let Some(header_len) = h3::client_transport::parse_webtransport_uni_header(buf) {
                let leftover = buf.split_off(header_len);
                session.pending_peer_uni_headers.remove(&id);
                session.peer_uni_stream_leftover.insert(id, leftover);
                ready.push(id);
            }
        }
        Ok(ready)
    }

    /// GAP-WEBTRANSPORT срез 4e: `incomingBidirectionalStreams`'s discovery
    /// primitive — the bidi counterpart of
    /// [`Self::webtransport_poll_incoming_uni_streams`]. Shares its discovery
    /// step ([`Self::classify_discovered_peer_streams`]) and its header-parse
    /// loop shape, but drains [`WebTransportSession::pending_peer_bidi_headers`]
    /// instead, and a returned id already has its send half registered (see
    /// [`Self::classify_discovered_peer_streams`]'s doc) — ready for both
    /// [`Self::webtransport_read_incoming_bidi_stream`] and, unchanged, the
    /// existing `webtransport_write_uni_stream`/`webtransport_close_uni_stream`/
    /// `webtransport_abort_uni_stream`.
    fn webtransport_poll_incoming_bidi_streams(&self, handle: i32) -> Result<Vec<u64>> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;

        Self::classify_discovered_peer_streams(session)?;

        let mut ready = Vec::new();
        let pending_ids: Vec<u64> = session.pending_peer_bidi_headers.keys().copied().collect();
        for id in pending_ids {
            let chunk =
                h3::client_transport::h3_webtransport_read_stream_on_driver(&mut session.driver, id)
                    .map_err(|e| Error::Network(format!("WebTransport poll incoming streams: {e}")))?;
            let buf = session.pending_peer_bidi_headers.entry(id).or_default();
            buf.extend_from_slice(&chunk);
            if let Some(header_len) = h3::client_transport::parse_webtransport_uni_header(buf) {
                let leftover = buf.split_off(header_len);
                session.pending_peer_bidi_headers.remove(&id);
                session.peer_bidi_stream_leftover.insert(id, leftover);
                ready.push(id);
            }
        }
        Ok(ready)
    }

    /// GAP-WEBTRANSPORT срез 4e: reads a peer-initiated bidirectional
    /// stream's bytes — the read half `incomingBidirectionalStreams` hands
    /// JS once [`Self::webtransport_poll_incoming_bidi_streams`] reports
    /// `stream_id` ready. Same "prepend the header-parse leftover, then drive
    /// the shared non-blocking read primitive" shape as
    /// [`Self::webtransport_read_incoming_uni_stream`].
    fn webtransport_read_incoming_bidi_stream(&self, handle: i32, stream_id: u64) -> Result<(Vec<u8>, bool)> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        let mut bytes = session.peer_bidi_stream_leftover.remove(&stream_id).unwrap_or_default();
        let chunk =
            h3::client_transport::h3_webtransport_read_stream_on_driver(&mut session.driver, stream_id)
                .map_err(|e| Error::Network(format!("WebTransport read incoming stream: {e}")))?;
        bytes.extend_from_slice(&chunk);
        let finished =
            h3::client_transport::h3_webtransport_stream_finished_on_driver(&session.driver, stream_id);
        Ok((bytes, finished))
    }

    /// GAP-WEBTRANSPORT срез 4d: reads a peer-initiated unidirectional
    /// stream's bytes — the read half `incomingUnidirectionalStreams` hands
    /// JS a `ReadableStream` for, once
    /// [`Self::webtransport_poll_incoming_uni_streams`] reports `stream_id`
    /// ready. Prepends any application bytes
    /// [`Self::webtransport_poll_incoming_uni_streams`] already read past the
    /// header while classifying the stream (only ever non-empty on the first
    /// call for a given `stream_id`), then drives the same non-blocking read
    /// primitive [`Self::webtransport_read_bidi_stream`] uses.
    fn webtransport_read_incoming_uni_stream(&self, handle: i32, stream_id: u64) -> Result<(Vec<u8>, bool)> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        let mut bytes = session.peer_uni_stream_leftover.remove(&stream_id).unwrap_or_default();
        let chunk =
            h3::client_transport::h3_webtransport_read_stream_on_driver(&mut session.driver, stream_id)
                .map_err(|e| Error::Network(format!("WebTransport read incoming stream: {e}")))?;
        bytes.extend_from_slice(&chunk);
        let finished =
            h3::client_transport::h3_webtransport_stream_finished_on_driver(&session.driver, stream_id);
        Ok((bytes, finished))
    }

    /// GAP-WEBTRANSPORT срез datagrams-b: sends `data` as a QUIC DATAGRAM
    /// (RFC 9221) on the session `handle` names —
    /// `WebTransportDatagramDuplexStream.writable`'s transport primitive.
    /// Delegates straight to
    /// [`h3::client_transport::h3_webtransport_send_datagram_on_driver`],
    /// which prefixes `data` with the session's RFC 9297 §2.1 quarter stream
    /// id before enqueuing and flushing.
    fn webtransport_send_datagram(&self, handle: i32, data: &[u8]) -> Result<()> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        h3::client_transport::h3_webtransport_send_datagram_on_driver(
            &mut session.driver,
            session.session_id,
            data,
        )
        .map_err(|e| Error::Network(format!("WebTransport send datagram: {e}")))
    }

    /// GAP-WEBTRANSPORT срез datagrams-b: `WebTransportDatagramDuplexStream.readable`'s
    /// non-blocking poll primitive — drains every QUIC DATAGRAM already
    /// queued on the session `handle` names' socket and returns the
    /// WebTransport application payload of each one whose RFC 9297 §2.1
    /// quarter stream id names this session, in arrival order. Unlike a
    /// stream, a datagram carries no `finished` signal (RFC 9221 datagrams
    /// are unreliable and unordered by the transport, delivered whole or not
    /// at all) — the caller polls again later rather than waiting on one.
    fn webtransport_poll_incoming_datagrams(&self, handle: i32) -> Result<Vec<Vec<u8>>> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        h3::client_transport::h3_webtransport_poll_incoming_datagrams_on_driver(
            &mut session.driver,
            session.session_id,
        )
        .map_err(|e| Error::Network(format!("WebTransport poll incoming datagrams: {e}")))
    }

    /// GAP-WEBTRANSPORT срез 5: `WebTransport.prototype.close(closeInfo)`'s
    /// transport primitive — sends a `CLOSE_WEBTRANSPORT_SESSION` capsule
    /// ([`h3::client_transport::h3_webtransport_close_session_on_driver`]) and
    /// unconditionally drops `handle`'s entry from
    /// [`Self::webtransport_sessions`] — a closed session has no further use
    /// for its driver regardless of whether the capsule reached the wire, and
    /// `close()` never surfaces a failure back to script (spec §5.4: it
    /// neither throws nor returns a rejectable value), so the caller (the JS
    /// shim, via `_lumen_webtransport_close_session`) only needs this to have
    /// no observable effect on a session that is already gone.
    fn webtransport_close_session(&self, handle: i32, close_code: u32, reason: &str) -> Result<()> {
        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut session) = sessions.remove(&handle) else {
            return Err(Error::Network("WebTransport session not found".to_string()));
        };
        let session_id = session.session_id;
        h3::client_transport::h3_webtransport_close_session_on_driver(
            &mut session.driver,
            session_id,
            close_code,
            reason.as_bytes(),
        )
        .map_err(|e| Error::Network(format!("WebTransport close session: {e}")))
    }

    /// GAP-WEBTRANSPORT, remaining sub-slice of срез 5: detects a **peer**-initiated
    /// close (a `CLOSE_WEBTRANSPORT_SESSION` capsule arriving on the session's
    /// own Extended CONNECT stream, read via the same
    /// [`h3::client_transport::h3_webtransport_read_stream_on_driver`] every
    /// other stream read uses — the session stream is not special-cased at
    /// the transport layer, only at this accumulate-and-decode layer) or an
    /// abrupt connection loss (any error surfacing from polling the socket).
    /// Either outcome removes `handle`'s entry from
    /// [`Self::webtransport_sessions`], same as a locally initiated close —
    /// a session with no further use for its driver either way.
    fn webtransport_poll_closed(&self, handle: i32) -> Result<lumen_core::ext::WebTransportSessionState> {
        use lumen_core::ext::WebTransportSessionState;

        let mut sessions = self.webtransport_sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::Network("WebTransport session not found".to_string()))?;
        let session_id = session.session_id;

        let bytes = match h3::client_transport::h3_webtransport_read_stream_on_driver(
            &mut session.driver,
            session_id,
        ) {
            Ok(bytes) => bytes,
            Err(_) => {
                sessions.remove(&handle);
                return Ok(WebTransportSessionState::ConnectionLost);
            }
        };
        session.pending_close_capsule.extend_from_slice(&bytes);

        if let Some((close_code, reason)) =
            h3::capsule::decode_close_webtransport_session(&session.pending_close_capsule)
        {
            sessions.remove(&handle);
            return Ok(WebTransportSessionState::ClosedByPeer {
                close_code,
                reason: String::from_utf8_lossy(&reason).into_owned(),
            });
        }

        Ok(WebTransportSessionState::Open)
    }
}

impl HttpClient {
    /// Shared discovery step for [`JsFetchProvider::webtransport_poll_incoming_uni_streams`]
    /// and [`JsFetchProvider::webtransport_poll_incoming_bidi_streams`]
    /// (GAP-WEBTRANSPORT срез 4e) — drains
    /// [`h3::client_transport::h3_webtransport_poll_new_peer_streams_on_driver`]
    /// once and files every newly discovered id into the map its direction
    /// owns ([`h3::stream::is_unidirectional`] → `pending_peer_uni_headers`,
    /// [`h3::stream::is_bidirectional`] → `pending_peer_bidi_headers`).
    ///
    /// Calling this from both poll methods on the same session is safe
    /// despite the shared drain: whichever call runs first for a given round
    /// of newly-arrived ids sees and classifies all of them (into either
    /// map, as appropriate); the other call finds nothing new from the drain
    /// but still processes whatever is already pending in its own map. No id
    /// is ever lost to whichever poll happens to run second.
    ///
    /// A bidirectional id also gets its send half registered right away
    /// ([`h3::client_transport::h3_webtransport_open_incoming_bidi_send_on_driver`])
    /// — unlike an incoming unidirectional stream, an incoming bidi stream's
    /// `writable` needs a live [`h3::stream::SendStream`] under the peer's own
    /// id before `write()`/`close()`/`abort()` on it (the pre-existing,
    /// stream-id-generic natives) can do anything.
    fn classify_discovered_peer_streams(session: &mut WebTransportSession) -> Result<()> {
        let discovered =
            h3::client_transport::h3_webtransport_poll_new_peer_streams_on_driver(&mut session.driver)
                .map_err(|e| Error::Network(format!("WebTransport poll incoming streams: {e}")))?;
        let peer_initial_max_stream_data_bidi = session.peer_initial_max_stream_data_bidi;
        for id in discovered {
            if h3::stream::is_unidirectional(id) {
                session.pending_peer_uni_headers.entry(id).or_default();
            } else {
                debug_assert!(h3::stream::is_bidirectional(id));
                h3::client_transport::h3_webtransport_open_incoming_bidi_send_on_driver(
                    &mut session.driver,
                    id,
                    peer_initial_max_stream_data_bidi,
                );
                session.pending_peer_bidi_headers.entry(id).or_default();
            }
        }
        Ok(())
    }

    /// `connect-src`/`default-src` gate shared by every JS-initiated network
    /// path that owns a parsed `Url` — `fetch_request_impl` below and
    /// [`JsFetchProvider::check_connect_src`]'s override (`sendBeacon`'s
    /// pre-spawn check, GAP-CSPENF срез 12).
    fn connect_src_gate(&self, url: &Url) -> Result<()> {
        if let Some((policies, self_origin, original_policy)) = &self.connect_src_policy
            && policies
                .iter()
                .any(|policy| !policy.fetch_directive_allows(&CspDirective::ConnectSrc, url, self_origin.as_ref()))
        {
            return Err(Error::CspConnectSrcBlocked {
                blocked_uri: url.to_string(),
                original_policy: original_policy.clone(),
            });
        }
        Ok(())
    }

    /// `worker-src`/`child-src`/`default-src` gate backing
    /// [`JsFetchProvider::check_worker_src`] (GAP-CSPENF срез 13, `child-src`
    /// fallback step added срез 26) — same shape as [`Self::connect_src_gate`],
    /// checked against [`Self::worker_src_policy`] and `CspDirective::WorkerSrc`
    /// instead.
    fn worker_src_gate(&self, url: &Url) -> Result<()> {
        if let Some((policies, self_origin, original_policy)) = &self.worker_src_policy
            && policies.iter().any(|policy| {
                !policy.fetch_directive_allows_via_child_src(&CspDirective::WorkerSrc, url, self_origin.as_ref())
            })
        {
            return Err(Error::CspWorkerSrcBlocked {
                blocked_uri: url.to_string(),
                original_policy: original_policy.clone(),
            });
        }
        Ok(())
    }

    /// `object-src`/`default-src` gate backing [`JsFetchProvider::check_object_src`]
    /// (GAP-CSPENF срез 16) — same shape as [`Self::worker_src_gate`], checked
    /// against [`Self::object_src_policy`] and `CspDirective::ObjectSrc` instead.
    fn object_src_gate(&self, url: &Url) -> Result<()> {
        if let Some((policies, self_origin, original_policy)) = &self.object_src_policy
            && policies
                .iter()
                .any(|policy| !policy.fetch_directive_allows(&CspDirective::ObjectSrc, url, self_origin.as_ref()))
        {
            return Err(Error::CspObjectSrcBlocked {
                blocked_uri: url.to_string(),
                original_policy: original_policy.clone(),
            });
        }
        Ok(())
    }

    /// `media-src`/`default-src` gate backing [`JsFetchProvider::check_media_src`]
    /// (GAP-CSPENF срез 17) — same shape as [`Self::object_src_gate`], checked
    /// against [`Self::media_src_policy`] and `CspDirective::MediaSrc` instead.
    fn media_src_gate(&self, url: &Url) -> Result<()> {
        if let Some((policies, self_origin, original_policy)) = &self.media_src_policy
            && policies
                .iter()
                .any(|policy| !policy.fetch_directive_allows(&CspDirective::MediaSrc, url, self_origin.as_ref()))
        {
            return Err(Error::CspMediaSrcBlocked {
                blocked_uri: url.to_string(),
                original_policy: original_policy.clone(),
            });
        }
        Ok(())
    }

    /// CSP gate of a `<link rel=preload|modulepreload|prefetch|icon>` fetch
    /// (BUG-1491): the destination `as_kind` names (`""` for prefetch) picks
    /// the directive ([`CspPolicy::preload_fetch_allows`]), checked against
    /// the document policies of [`Self::element_src_policy`]. The URL is
    /// upgraded first, as the request will be sent.
    pub(crate) fn preload_src_gate(&self, as_kind: &str, url: &str, nonce: &str) -> Result<()> {
        let Some((policies, self_origin, original_policy)) = &self.element_src_policy else {
            return Ok(());
        };
        let parsed = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        let parsed = self.upgrade_insecure_requests_url(parsed);
        let nonce = Some(nonce).filter(|n| !n.is_empty());
        if policies
            .iter()
            .all(|policy| policy.preload_fetch_allows(as_kind, &parsed, self_origin.as_ref(), nonce))
        {
            return Ok(());
        }
        Err(Error::CspElementSrcBlocked {
            directive: csp_directive_name_for_preload(as_kind).to_owned(),
            blocked_uri: parsed.to_string(),
            original_policy: original_policy.clone(),
        })
    }

    /// `script-src`/`style-src` gate backing [`JsFetchProvider::check_element_src`]
    /// (BUG-1175) — same shape as [`Self::media_src_gate`], but the check is
    /// the element pre-request one: nonce (and, for a script, integrity and
    /// `'strict-dynamic'`) before the URL. Any destination other than
    /// `script`/`style` passes. `parser_inserted` — a `<script src>` that
    /// `document.write()` wrote (BUG-568), which `'strict-dynamic'` does not
    /// admit.
    fn element_src_gate(
        &self,
        destination: &str,
        url: &Url,
        nonce: &str,
        integrity: &str,
        parser_inserted: bool,
    ) -> Result<()> {
        let Some((policies, self_origin, original_policy)) = &self.element_src_policy else {
            return Ok(());
        };
        let nonce = Some(nonce).filter(|n| !n.is_empty());
        let (directive, blocked) = match destination {
            "script" => {
                let request = csp::ScriptRequestMetadata {
                    nonce,
                    integrity: Some(integrity).filter(|i| !i.is_empty()),
                    parser_inserted,
                };
                let blocked = policies
                    .iter()
                    .any(|policy| !policy.script_element_fetch_allows(url, self_origin.as_ref(), &request));
                ("script-src-elem", blocked)
            }
            "style" => {
                let blocked = policies
                    .iter()
                    .any(|policy| !policy.style_element_fetch_allows(url, self_origin.as_ref(), nonce));
                ("style-src-elem", blocked)
            }
            _ => return Ok(()),
        };
        if blocked {
            return Err(Error::CspElementSrcBlocked {
                directive: directive.to_owned(),
                blocked_uri: url.to_string(),
                original_policy: original_policy.clone(),
            });
        }
        Ok(())
    }

    /// GAP-CSPENF срез 49: `upgrade-insecure-requests` for JS-initiated
    /// network requests — the last item срез 48 named not covered
    /// (`fetch()`/`XMLHttpRequest`/WebSocket/EventSource; `sendBeacon` is a
    /// thin wrapper over `fetch_with_body_sync`, which funnels through
    /// [`Self::fetch_request_impl`] below, so it is covered for free).
    ///
    /// Duplicates the scheme-swap that `lumen_shell::csp_enforce::
    /// upgrade_insecure_url` already applies to parser-driven subresources
    /// (срезы 43-48) instead of depending on it: `lumen-network` sits below
    /// `lumen-shell` in the crate graph and cannot import it. The policies
    /// needed already live on `HttpClient` — [`Self::connect_src_policy`] is
    /// the same `Vec<CspPolicy>` `with_connect_src_policy` stores for every
    /// one of these call sites, so no new field is needed.
    ///
    /// Per [UIR] §4.1 step 5, upgrading must happen before the `connect-src`
    /// block check (step 6) — every call site below calls this immediately
    /// after `Url::parse`, before its gate. `ws`/`wss` are handled alongside
    /// `http`/`https` because a WebSocket handshake is a `connect-src`-gated
    /// fetch under CSP3 §6.7.2 just like `fetch()`, and the Mixed Content
    /// upgrade algorithm this directive defers to treats both pairs alike.
    ///
    /// [UIR]: https://w3c.github.io/webappsec-upgrade-insecure-requests/
    pub(crate) fn upgrade_insecure_requests_url(&self, url: Url) -> Url {
        let Some((policies, _, _)) = &self.connect_src_policy else {
            return url;
        };
        if !policies.iter().any(|p| p.upgrade_insecure_requests) {
            return url;
        }
        let new_scheme = match url.scheme() {
            "http" => "https",
            "ws" => "wss",
            _ => return url,
        };
        let serialized = url.as_str().to_string();
        let Some((_, rest)) = serialized.split_once(':') else {
            return url;
        };
        Url::parse(&format!("{new_scheme}:{rest}")).unwrap_or(url)
    }

    /// Общее тело JS-запроса. `allow_sw_intercept = false` — для запросов,
    /// исходящих из самого service worker-а.
    fn fetch_request_impl(
        &self,
        req: &JsFetchRequest<'_>,
        allow_sw_intercept: bool,
    ) -> Result<JsFetchResult> {
        let url = Url::parse(req.url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        let url = self.upgrade_insecure_requests_url(url);
        // BUG-1175: an element loading itself through the shim is not a
        // `connect-src` request — CSP3 judges a `script`/`style` destination
        // by `script-src`/`style-src`, which the shim has already asked
        // `check_element_src` about (only it knows the element's nonce).
        if !matches!(req.destination, "script" | "style") {
            self.connect_src_gate(&url)?;
        }
        let method_upper = req.method.to_ascii_uppercase();
        match (req.body.is_some(), method_upper.as_str()) {
            (false, "GET" | "HEAD") | (true, "POST" | "PUT" | "PATCH" | "DELETE") => {}
            (false, m) => {
                return Err(Error::Network(format!(
                    "fetch: Phase 0 supports GET/HEAD only, got {m}"
                )));
            }
            (true, m) => {
                return Err(Error::Network(format!(
                    "fetch_with_body: unsupported method {m}"
                )));
            }
        }
        // Предварительная отмена (AbortSignal, уже сработавший до отправки) и
        // установка токена на поток: `do_request` глубоко в пути чтения поднимет
        // по нему watchdog и порвёт сокет, если abort() случится в полёте.
        let _scope = match req.token {
            Some(t) if t.is_aborted() => {
                return Err(Error::Aborted("fetch aborted before send".to_string()));
            }
            Some(t) => Some(AbortScope::new(t.clone())),
            None => None,
        };
        // SW intercept before network — только для запросов без тела (GET/HEAD):
        // CacheStorage адресуется URL-ом, тело в ключ не входит.
        if allow_sw_intercept
            && req.body.is_none()
            && let Some(ref interceptor) = self.interceptor
        {
            let origin = build_origin(&url);
            if let Some(body) = interceptor.intercept(&url, &origin) {
                return Ok(JsFetchResult {
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![],
                    body,
                    url: url.to_string(),
                });
            }
        }
        // Тот же путь, что у GET: куки, редиректы, HSTS, ad-block-фильтр,
        // mixed-content, HTTP/2, retry на протухшем keep-alive. Своя ветка
        // «сокет + write + read» для тела стояла здесь до 2026-08-17 и не имела
        // ничего из перечисленного — POST уходил без Cookie, не ходил по
        // Location и писал HTTP/1.1-байты в согласованное h2-соединение.
        let request_body = req.body.as_ref().map(|b| RequestBody {
            method: &method_upper,
            content_type: b.content_type,
            bytes: b.bytes,
        });
        let mut author_headers = build_author_headers(req.headers, request_body.is_some());
        // GAP-REFERRER срез 1: `Referer` (Referrer Policy §8.3) and, for
        // non-GET/HEAD, `Origin` (Fetch §"append a request's Origin header")
        // — computed from the document context attached at construction time
        // (`with_document_context`), not from `req.headers`: both are on the
        // forbidden author-header list (`build_author_headers` already drops
        // them), so a page can never spoof either through `fetch()`/
        // `setRequestHeader`.
        if let Some((referrer_url, policy)) = &self.document_context {
            if let Some(referer) = referrer_policy::compute_referrer(*policy, referrer_url, &url) {
                author_headers.push_str("Referer: ");
                author_headers.push_str(&referer);
                author_headers.push_str("\r\n");
            }
            if method_upper != "GET"
                && method_upper != "HEAD"
                && let Ok(doc_origin) = Origin::from_url(referrer_url)
            {
                author_headers.push_str("Origin: ");
                author_headers.push_str(&doc_origin.serialize());
                author_headers.push_str("\r\n");
            }
        }
        // BUG-1021: the request's own mode instead of the profile's
        // navigation block — a plain `fetch()`/XHR is `cors`/`empty`, an
        // element loading itself through the shim (`@import`) names its own
        // `no-cors` mode and destination. A cross-origin `cors` GET/HEAD gets
        // `Origin` too; non-GET/HEAD already has it from the branch above.
        let doc_url = self.document_context.as_ref().map(|(u, _)| u);
        let mode = fetch_metadata::RequestMode::from_fetch_mode(req.mode);
        if method_upper == "GET" || method_upper == "HEAD" {
            author_headers.push_str(&fetch_metadata::cors_origin_header(doc_url, &url, mode));
        }
        author_headers.push_str(&fetch_metadata::fetch_metadata_headers(
            self.fingerprint_profile,
            doc_url,
            &url,
            fetch_metadata::dest_token(req.destination),
            mode,
        ));
        let accept_encoding = self.accept_encoding_header();
        let destination = self.mixed_content.as_ref().map(|_| RequestDestination::Other);
        let (resp, final_url) = fetch_with_redirect(
            &url,
            5,
            &self.pool,
            self.h2_pool.as_deref(),
            self.resolver.as_ref(),
            self.tls_profile,
            self.fingerprint_profile,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.hsts.as_deref(),
            self.credentials.as_deref(),
            &self.decoders,
            accept_encoding.as_deref(),
            None,
            None,
            self.tab_id,
            self.mixed_content.as_ref(),
            destination,
            None,
            &author_headers,
            self.cookie_jar.as_deref(),
            self.top_level_site.as_deref(),
            self.document_host(),
            self.proxy.as_deref(),
            self.socks5_proxy.as_deref(),
            self.h3_alt_svc(),
            self.h3_pool(),
            None,  // PH1-2a: streaming sink — only fetch_page_streaming streams
            false, // BUG-292: subresource, not a document navigation
            request_body.as_ref(),
            &method_upper, // BUG-1665: метод fetch()-запроса (GET/HEAD)
            true, // BUG-1114: fetch() resolves ok:false on 4xx/5xx, doesn't reject
        )?;
        Ok(JsFetchResult {
            status_text: http_status_text(resp.status).to_string(),
            status: resp.status,
            headers: resp
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            body: resp.body,
            url: final_url.to_string(),
        })
    }

}

impl WebSocketProvider for HttpClient {
    fn connect_ws(
        &self,
        url: &Url,
        tab_id: TabId,
        sink: Arc<dyn EventSink>,
    ) -> Result<Box<dyn WebSocketSession>> {
        let ws = websocket::WebSocket::connect(
            url,
            self.resolver.as_ref(),
            self.hsts.as_deref(),
            sink,
            tab_id,
            &[],
        )?;
        Ok(Box::new(ws))
    }
}

impl SseProvider for HttpClient {
    fn connect_sse(
        &self,
        url: &Url,
        tab_id: TabId,
        sink: Arc<dyn EventSink>,
    ) -> Result<Box<dyn SseSession>> {
        let cookies = self.cookie_jar.as_ref().map(|jar| sse::SseCookies {
            jar: Arc::clone(jar),
            document_host: self.document_host().map(str::to_owned),
        });
        let es = sse::EventSource::connect(url, Arc::clone(&self.resolver), sink, tab_id, cookies)?;
        Ok(Box::new(es))
    }
}

// ── JsWebSocketSession ────────────────────────────────────────────────────────

/// How long the background recv thread waits for a frame before releasing
/// `session` and checking again (BUG-307). Bounds the worst-case latency a
/// JS-invoked `send`/`close` call can be blocked behind an idle connection's
/// reader lock — was previously unbounded (the reader held the lock inside
/// a plain, timeout-less blocking `recv()`), which froze the single-threaded
/// JS engine solid (and with it the whole UI thread, via `EngineThread::query`)
/// whenever a page called any WebSocket method while its socket was idle.
pub(crate) const WS_RECV_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// One not-yet-written outgoing message, queued by `send_text`/`send_binary`
/// (GAP-WSASYNC срез 2, BUG-869) so the JS thread never blocks on the socket.
enum QueuedWsFrame {
    Text(String),
    Binary(Vec<u8>),
}

/// Background-threaded WebSocket session for the JS runtime.
///
/// Spawns a receive thread that pushes `JsWsEvent`s into a shared queue.
/// JS calls `poll()` to drain the queue without blocking the script thread.
///
/// Sends go through the same asymmetry: `send_text`/`send_binary` only queue
/// the frame and return, a background writer thread performs the actual
/// blocking `write` (BUG-869 — a slow reader on the other end used to stall
/// the write inside the JS thread, freezing the whole document for as long
/// as the peer took to drain its TCP receive buffer).
struct JsWebSocketSessionImpl {
    /// For sending: shared so this struct, the writer thread and (via
    /// `close()`) the JS thread can all reach the same underlying stream.
    session: Arc<std::sync::Mutex<Box<dyn WebSocketSession>>>,
    /// Buffered events produced by the background recv/writer threads.
    queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsWsEvent>>>,
    /// Outgoing messages not yet handed to `send_frame` by the writer thread.
    send_queue: Arc<std::sync::Mutex<std::collections::VecDeque<QueuedWsFrame>>>,
    /// `false` once the recv thread has observed Close/Error — tells the
    /// writer thread it can stop polling `send_queue` once it drains empty,
    /// instead of parking forever on a socket nothing will ever read again.
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Server-negotiated sub-protocol, cached at connect time — never changes
    /// afterwards, so `protocol()` doesn't need to contend with the recv
    /// thread's `session` lock at all (BUG-307).
    protocol: String,
}

impl JsWebSocketSessionImpl {
    /// Create a new session, spawning background threads to receive frames
    /// and to write queued outgoing ones.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn new(ws: websocket::WebSocket) -> Self {
        let protocol = ws.protocol().to_string();
        let queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsWsEvent>>> =
            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let session: Arc<std::sync::Mutex<Box<dyn WebSocketSession>>> =
            Arc::new(std::sync::Mutex::new(Box::new(ws)));
        let send_queue: Arc<std::sync::Mutex<std::collections::VecDeque<QueuedWsFrame>>> =
            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let q2 = Arc::clone(&queue);
        let s2 = Arc::clone(&session);
        let running_recv = Arc::clone(&running);

        // The background thread polls recv_timeout() in a loop and pushes
        // events into the shared queue so JS can poll without blocking. Each
        // poll only holds `session` for up to WS_RECV_POLL_INTERVAL, then
        // releases it — see BUG-307: an unbounded lock-and-block here starved
        // any concurrent JS-invoked send_text/send_binary/close/protocol call
        // (all of which lock the same `session`) for as long as the socket
        // stayed idle, i.e. forever on a connection with no pending traffic.
        std::thread::spawn(move || {
            loop {
                let result = s2.lock().unwrap().recv_timeout(WS_RECV_POLL_INTERVAL);
                match result {
                    Ok(None) => {
                        // Nothing arrived within the poll window — the lock
                        // was already released above (the temporary guard
                        // from `s2.lock().unwrap()` drops at the end of the
                        // previous statement), but `std::sync::Mutex` gives
                        // no fairness guarantee: relocking immediately here
                        // can starve a JS-invoked send_text/send_binary/close
                        // waiting on the same `session` (observed: it can
                        // lose the race for several seconds straight under
                        // load). Sleep briefly outside the lock so the OS
                        // scheduler has a genuine window to hand it to a
                        // waiter instead of handing it straight back to us.
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Ok(Some(lumen_core::ext::WsMessage::Text(t))) => {
                        q2.lock().unwrap().push_back(JsWsEvent::Message {
                            data: t.into_bytes(),
                            is_binary: false,
                        });
                    }
                    Ok(Some(lumen_core::ext::WsMessage::Binary(b))) => {
                        q2.lock().unwrap().push_back(JsWsEvent::Message {
                            data: b,
                            is_binary: true,
                        });
                    }
                    Ok(Some(lumen_core::ext::WsMessage::Close { code, reason })) => {
                        q2.lock()
                            .unwrap()
                            .push_back(JsWsEvent::Close { code, reason });
                        running_recv.store(false, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    Ok(Some(
                        lumen_core::ext::WsMessage::Ping(_)
                        | lumen_core::ext::WsMessage::Pong(_),
                    )) => {
                        // Control frames handled internally by WebSocket::recv_inner.
                    }
                    Err(e) => {
                        q2.lock()
                            .unwrap()
                            .push_back(JsWsEvent::Error(e.to_string()));
                        running_recv.store(false, std::sync::atomic::Ordering::Release);
                        break;
                    }
                }
            }
        });

        let q3 = Arc::clone(&queue);
        let s3 = Arc::clone(&session);
        let sq2 = Arc::clone(&send_queue);
        let running_send = Arc::clone(&running);

        // Writer thread: drains `send_queue` in FIFO order, one message at a
        // time, performing the actual blocking socket write off the JS
        // thread. A `Flushed` event reports each message's application-data
        // length back to JS so `bufferedAmount` can be decremented as data
        // actually leaves — see `send_text`/`send_binary` below for the
        // matching increment. Exits once the connection is no longer
        // `running` and the queue has drained (nothing left to flush).
        std::thread::spawn(move || {
            loop {
                let next = sq2.lock().unwrap().pop_front();
                let Some(frame) = next else {
                    if !running_send.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                };
                let (result, bytes) = match frame {
                    QueuedWsFrame::Text(text) => {
                        let n = text.len() as u64;
                        (s3.lock().unwrap().send_text(&text), n)
                    }
                    QueuedWsFrame::Binary(data) => {
                        let n = data.len() as u64;
                        (s3.lock().unwrap().send_binary(&data), n)
                    }
                };
                match result {
                    Ok(()) => {
                        q3.lock().unwrap().push_back(JsWsEvent::Flushed { bytes });
                    }
                    Err(e) => {
                        q3.lock()
                            .unwrap()
                            .push_back(JsWsEvent::Error(e.to_string()));
                        running_send.store(false, std::sync::atomic::Ordering::Release);
                        break;
                    }
                }
            }
        });

        Self { session, queue, send_queue, running, protocol }
    }
}

impl JsWebSocketSession for JsWebSocketSessionImpl {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn send_text(&self, text: &str) -> Result<()> {
        self.send_queue
            .lock()
            .unwrap()
            .push_back(QueuedWsFrame::Text(text.to_string()));
        Ok(())
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn send_binary(&self, data: &[u8]) -> Result<()> {
        self.send_queue
            .lock()
            .unwrap()
            .push_back(QueuedWsFrame::Binary(data.to_vec()));
        Ok(())
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn poll(&self) -> Option<JsWsEvent> {
        self.queue.lock().unwrap().pop_front()
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn close(&self, code: u16, reason: &str) -> Result<()> {
        self.running.store(false, std::sync::atomic::Ordering::Release);
        // GAP-WSASYNC срез 3 (BUG-869): `session.close()` blocks on the same
        // mutex the writer thread can be holding for the duration of a
        // blocking `send_text`/`send_binary` under backpressure — calling it
        // straight from the JS thread (as this used to) stalled the whole
        // document for however long that write took. Run it on a detached
        // thread instead: the caller (`_lumen_ws_close`) already discards
        // this method's `Result`, and the shim has already flipped
        // `readyState` to CLOSING before invoking it, so nothing observable
        // depends on the close frame having gone out by the time this
        // returns.
        let session = Arc::clone(&self.session);
        let reason = reason.to_string();
        std::thread::spawn(move || {
            let _ = session.lock().unwrap().close(code, &reason);
        });
        Ok(())
    }

    fn protocol(&self) -> String {
        self.protocol.clone()
    }
}

impl JsWebSocketProvider for HttpClient {
    fn connect(&self, url: &str, protocols: &[String]) -> Result<Box<dyn JsWebSocketSession>> {
        self.connect_ws_impl(url, protocols)
    }

    /// GAP-WSASYNC срез 4 (BUG-856): installs `token` on this thread via
    /// [`AbortScope`] for the duration of the (synchronous) handshake —
    /// `websocket::WebSocket::connect_deflate` reads it back via
    /// `current_abort_token()` and spawns an `AbortWatchdog` around the
    /// blocking Upgrade read, exactly like `do_request` does for an
    /// in-flight fetch. This is the caller-visible half of `close()` during
    /// `CONNECTING` no longer waiting out `FETCH_READ_TIMEOUT`.
    fn connect_cancellable(
        &self,
        url: &str,
        protocols: &[String],
        token: &AbortToken,
    ) -> Result<Box<dyn JsWebSocketSession>> {
        if token.is_aborted() {
            return Err(Error::Aborted("ws: connect aborted".to_string()));
        }
        let _scope = AbortScope::new(token.clone());
        self.connect_ws_impl(url, protocols)
    }
}

impl HttpClient {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn connect_ws_impl(&self, url: &str, protocols: &[String]) -> Result<Box<dyn JsWebSocketSession>> {
        let parsed = Url::parse(url)
            .map_err(|e| Error::Network(format!("ws: invalid URL: {e}")))?;
        // GAP-CSPENF срез 49: upgrade `ws:` to `wss:` before the gate below —
        // same UIR §4.1 step 5-before-6 ordering as `fetch_request_impl`.
        let parsed = self.upgrade_insecure_requests_url(parsed);
        // GAP-CSPENF срез 11: same one-time `connect_src_policy` gate that
        // `fetch_request_impl` (срез 10) checks before any socket work — a
        // WebSocket handshake is a `connect-src`-gated fetch (CSP3 §6.7.2) just
        // like `fetch()`/XHR, it just does not share their call path.
        if let Some((policies, self_origin, original_policy)) = &self.connect_src_policy
            && policies
                .iter()
                .any(|policy| !policy.fetch_directive_allows(&CspDirective::ConnectSrc, &parsed, self_origin.as_ref()))
        {
            return Err(Error::CspConnectSrcBlocked {
                blocked_uri: parsed.to_string(),
                original_policy: original_policy.clone(),
            });
        }
        // Always offer permessage-deflate (RFC 7692) — browsers do this by default.
        // compress=false: outgoing frames are uncompressed until JS sets WebSocket.compress.
        let ws = websocket::WebSocket::connect_deflate(
            &parsed,
            self.resolver.as_ref(),
            self.hsts.as_deref(),
            Arc::new(NoopEventSink),
            lumen_core::event::TabId(0),
            false,
            protocols,
        )?;
        let impl_ = JsWebSocketSessionImpl::new(ws);
        // Push the Open event immediately — handshake already completed.
        impl_.queue.lock().unwrap().push_back(JsWsEvent::Open);
        Ok(Box::new(impl_))
    }
}

// ── JsSseSession ──────────────────────────────────────────────────────────────

/// Background-threaded SSE session for the JS runtime.
///
/// Spawns a receive thread that drains the blocking [`SseSession::next_event`]
/// loop and pushes [`JsSseEvent`]s into a shared queue. JS calls `poll()` to
/// drain the queue without blocking the script thread — mirroring
/// [`JsWebSocketSessionImpl`].
pub(crate) struct JsSseSessionImpl {
    /// Buffered events produced by the background recv thread.
    queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsSseEvent>>>,
    /// Cancellation handle shared with the underlying [`SseSession`]. Signalling
    /// it stops the reconnect loop and wakes a pending reconnect sleep so
    /// `close()` takes effect within one tick instead of after the full retry
    /// delay.
    cancel: lumen_core::ext::SseCancel,
}

/// [`EventSink`] that turns an [`EventSource`](sse) connection's *control*
/// events into queue entries for the JS session.
///
/// Messages deliberately do not go through here — they reach the queue via
/// [`SseSession::next_event`] on the recv thread, and duplicating them would
/// double every event. What only the sink can report is the connection
/// lifecycle: a (re)connection announced and a stream that ended. Both are
/// emitted synchronously from inside `next_event`, i.e. on that same thread, so
/// their position relative to the messages is preserved (BUG-844).
struct JsSseControlSink {
    /// The session queue, shared with [`JsSseSessionImpl`].
    queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsSseEvent>>>,
}

impl lumen_core::ext::EventSink for JsSseControlSink {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn emit(&self, event: &Event) {
        let out = match event {
            Event::SseConnected { .. } => JsSseEvent::Open,
            Event::SseError { .. } => JsSseEvent::Reconnecting,
            _ => return,
        };
        self.queue.lock().unwrap().push_back(out);
    }
}

impl JsSseSessionImpl {
    /// Create a new session over `queue`, spawning a background thread that
    /// buffers events.
    ///
    /// `queue` is the same one [`JsSseControlSink`] writes into and already
    /// carries the [`JsSseEvent::Open`] of the initial connection — the
    /// handshake ran before this call, so pushing an `Open` here would double
    /// it. The thread forwards every server event until the loop is cancelled
    /// ([`JsSseEvent::Close`]) or fails ([`JsSseEvent::Error`]); reconnection
    /// happens inside `next_event` and is reported through the sink.
    /// `close()` signals the shared cancel handle: it wakes a pending reconnect
    /// delay immediately, and the loop also checks it before each read so an
    /// idle session exits promptly. A read already blocked in the socket
    /// finishes naturally when the server closes.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn new(
        mut session: Box<dyn SseSession>,
        queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsSseEvent>>>,
    ) -> Self {
        let cancel = session.cancel();
        let cancel_bg = cancel.clone();

        let q2 = Arc::clone(&queue);

        std::thread::spawn(move || {
            loop {
                if cancel_bg.is_cancelled() {
                    session.close();
                    q2.lock().unwrap().push_back(JsSseEvent::Close);
                    break;
                }
                match session.next_event() {
                    Ok(Some(ev)) => {
                        let mut q = q2.lock().unwrap();
                        if let Some(ms) = ev.retry_ms {
                            q.push_back(JsSseEvent::Retry(ms));
                        }
                        q.push_back(JsSseEvent::Message {
                            event_type: ev.event_type,
                            data: ev.data,
                            id: ev.id,
                        });
                    }
                    Ok(None) => {
                        q2.lock().unwrap().push_back(JsSseEvent::Close);
                        break;
                    }
                    Err(e) => {
                        q2.lock().unwrap().push_back(JsSseEvent::Error(e.to_string()));
                        break;
                    }
                }
            }
        });

        Self { queue, cancel }
    }
}

impl JsSseSession for JsSseSessionImpl {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn poll(&self) -> Option<JsSseEvent> {
        self.queue.lock().unwrap().pop_front()
    }

    fn close(&mut self) {
        self.cancel.signal();
    }
}

impl JsSseProvider for HttpClient {
    fn connect_sse(&self, url: &str) -> Result<Box<dyn JsSseSession>> {
        let parsed = Url::parse(url)
            .map_err(|e| Error::Network(format!("sse: invalid URL: {e}")))?;
        // GAP-CSPENF срез 49: same `http` → `https` upgrade as `fetch_request_impl`
        // — EventSource speaks plain HTTP(S), not its own scheme, before the gate below.
        let parsed = self.upgrade_insecure_requests_url(parsed);
        // GAP-CSPENF срез 11: same `connect_src_policy` gate as `connect()`
        // above (WebSocket) — `EventSource` is `connect-src`-gated per CSP3
        // §6.7.2 and shares the same `HttpClient`-owned policy slot.
        if let Some((policies, self_origin, original_policy)) = &self.connect_src_policy
            && policies
                .iter()
                .any(|policy| !policy.fetch_directive_allows(&CspDirective::ConnectSrc, &parsed, self_origin.as_ref()))
        {
            return Err(Error::CspConnectSrcBlocked {
                blocked_uri: parsed.to_string(),
                original_policy: original_policy.clone(),
            });
        }
        // The queue is built BEFORE the handshake so the sink can record the
        // initial `Open` from inside it — the same path every later reconnection
        // takes, instead of a hand-pushed first event that no reconnection has.
        let queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsSseEvent>>> =
            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        // Reuse the synchronous SseProvider path; the EventSource handshake runs
        // here, then the background thread takes over event delivery.
        let session = <Self as SseProvider>::connect_sse(
            self,
            &parsed,
            lumen_core::event::TabId(0),
            Arc::new(JsSseControlSink {
                queue: Arc::clone(&queue),
            }),
        )?;
        Ok(Box::new(JsSseSessionImpl::new(session, queue)))
    }
}
