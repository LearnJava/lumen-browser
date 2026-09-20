    #![allow(clippy::unwrap_used)]

    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    // Full `install_dom`, not just `DOM_EXCEPTION_POLYFILL`: the constructor
    // validates the URL through the real `URL` class (`WEB_API_SHIM`'s
    // `url_shim.js`), which itself reads `_LUMEN_PAGE_URL` seeded by
    // `install_dom` — a bare runtime has no `URL` global at all.
    fn rt_with_webtransport() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false).unwrap();
        super::install_webtransport_v8(&rt, None).unwrap();
        rt
    }

    /// Queued answers [`StubFetch::webtransport_read_bidi_stream`] hands back
    /// in order, one per call.
    type ReadStreamResults = std::sync::Mutex<
        std::collections::VecDeque<lumen_core::error::Result<(Vec<u8>, bool)>>,
    >;

    /// A fetch provider whose `webtransport_connect` answers deterministically
    /// (`Ok`/`Err`) instead of network I/O — GAP-WEBTRANSPORT срез 2b. Its
    /// `webtransport_open_uni_stream` (срез 3b) and `webtransport_write_uni_stream`
    /// (срез 3c) are each a fixed result rather than driving a live handle.
    struct StubFetch {
        result: std::sync::Mutex<Option<lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>>>,
        uni_stream_result: lumen_core::error::Result<u64>,
        /// GAP-WEBTRANSPORT срез 4b: same shape as `uni_stream_result`, for
        /// `webtransport_open_bidi_stream`.
        bidi_stream_result: lumen_core::error::Result<u64>,
        write_stream_result: lumen_core::error::Result<()>,
        /// The `(handle, stream_id, bytes)` triple the last `write` call
        /// received, if any — lets a test assert the JS layer forwarded the
        /// right stream id and payload, not just that it resolved.
        last_write: std::sync::Mutex<Option<(i32, u64, Vec<u8>)>>,
        close_stream_result: lumen_core::error::Result<()>,
        abort_stream_result: lumen_core::error::Result<()>,
        /// The `(handle, stream_id)` pair the last `close` call received, if
        /// any — GAP-WEBTRANSPORT срез 3d.
        last_close: std::sync::Mutex<Option<(i32, u64)>>,
        /// The `(handle, stream_id, error_code)` triple the last `abort` call
        /// received, if any — GAP-WEBTRANSPORT срез 3d.
        last_abort: std::sync::Mutex<Option<(i32, u64, u64)>>,
        /// GAP-WEBTRANSPORT срез 4c: the queued answers
        /// `webtransport_read_bidi_stream` hands back in order, one per call
        /// (`pop_front`) — lets a test script a multi-poll sequence (bytes
        /// then finished, or an error). Empty defaults to `Ok((vec![], true))`
        /// so a test that never touches `readable` still terminates the
        /// shim's poll loop instead of spinning.
        read_stream_results: ReadStreamResults,
        /// GAP-WEBTRANSPORT срез 4d: same queued-answer shape as
        /// `read_stream_results`, for `webtransport_poll_incoming_uni_streams`.
        /// Empty defaults to `Ok(vec![])` so a test that never touches
        /// `incomingUnidirectionalStreams` still terminates the shim's
        /// discovery loop instead of spinning.
        poll_incoming_uni_results:
            std::sync::Mutex<std::collections::VecDeque<lumen_core::error::Result<Vec<u64>>>>,
        /// GAP-WEBTRANSPORT срез 4d: same queued-answer shape as
        /// `read_stream_results`, for `webtransport_read_incoming_uni_stream`.
        read_incoming_uni_results: ReadStreamResults,
        /// GAP-WEBTRANSPORT срез 4e: same queued-answer shape as
        /// `poll_incoming_uni_results`, for `webtransport_poll_incoming_bidi_streams`.
        poll_incoming_bidi_results:
            std::sync::Mutex<std::collections::VecDeque<lumen_core::error::Result<Vec<u64>>>>,
        /// GAP-WEBTRANSPORT срез 4e: same queued-answer shape as
        /// `read_stream_results`, for `webtransport_read_incoming_bidi_stream`.
        read_incoming_bidi_results: ReadStreamResults,
        /// GAP-WEBTRANSPORT срез datagrams-b: same shape as
        /// `write_stream_result`, for `webtransport_send_datagram`.
        send_datagram_result: lumen_core::error::Result<()>,
        /// The `(handle, bytes)` pair the last `webtransport_send_datagram`
        /// call received, if any — lets a test assert the JS layer forwarded
        /// the right session handle and payload, same purpose as
        /// `last_write`.
        last_send_datagram: std::sync::Mutex<Option<(i32, Vec<u8>)>>,
        /// GAP-WEBTRANSPORT срез datagrams-b: same queued-answer shape as
        /// `poll_incoming_uni_results`, for `webtransport_poll_incoming_datagrams`.
        /// Empty defaults to `Ok(vec![])` so a test that never touches
        /// `datagrams.readable` still terminates the shim's poll loop
        /// instead of spinning.
        poll_incoming_datagrams_results:
            std::sync::Mutex<std::collections::VecDeque<lumen_core::error::Result<Vec<Vec<u8>>>>>,
        /// GAP-WEBTRANSPORT срез 5: same shape as `write_stream_result`, for
        /// `webtransport_close_session`.
        close_session_result: lumen_core::error::Result<()>,
        /// The `(handle, closeCode, reason)` triple the last
        /// `webtransport_close_session` call received, if any — same purpose
        /// as `last_write`.
        last_close_session: std::sync::Mutex<Option<(i32, u32, String)>>,
        /// GAP-WEBTRANSPORT, remaining sub-slice of срез 5: same queued-answer
        /// shape as `poll_incoming_uni_results`, for `webtransport_poll_closed`.
        /// Empty defaults to `Ok(WebTransportSessionState::Open)` so a test
        /// that never touches peer-close detection still terminates
        /// `pollSessionClosedByPeer`'s loop (via a subsequent local `close()`)
        /// instead of spinning forever on "still open".
        poll_closed_results: std::sync::Mutex<
            std::collections::VecDeque<lumen_core::error::Result<lumen_core::ext::WebTransportSessionState>>,
        >,
    }
    impl lumen_core::ext::JsFetchProvider for StubFetch {
        fn fetch_sync(&self, _url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            Err(lumen_core::error::Error::Network("unused in this test".to_string()))
        }
        fn webtransport_connect(&self, _url: &str) -> lumen_core::error::Result<lumen_core::ext::JsWebTransportSession> {
            match self.result.lock().unwrap().take() {
                Some(r) => r,
                None => Err(lumen_core::error::Error::Network("StubFetch called twice".to_string())),
            }
        }
        fn webtransport_open_uni_stream(&self, _handle: i32) -> lumen_core::error::Result<u64> {
            match &self.uni_stream_result {
                Ok(id) => Ok(*id),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_open_bidi_stream(&self, _handle: i32) -> lumen_core::error::Result<u64> {
            match &self.bidi_stream_result {
                Ok(id) => Ok(*id),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_write_uni_stream(
            &self,
            handle: i32,
            stream_id: u64,
            data: &[u8],
        ) -> lumen_core::error::Result<()> {
            *self.last_write.lock().unwrap() = Some((handle, stream_id, data.to_vec()));
            match &self.write_stream_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_close_uni_stream(&self, handle: i32, stream_id: u64) -> lumen_core::error::Result<()> {
            *self.last_close.lock().unwrap() = Some((handle, stream_id));
            match &self.close_stream_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_abort_uni_stream(
            &self,
            handle: i32,
            stream_id: u64,
            error_code: u64,
        ) -> lumen_core::error::Result<()> {
            *self.last_abort.lock().unwrap() = Some((handle, stream_id, error_code));
            match &self.abort_stream_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_read_bidi_stream(
            &self,
            _handle: i32,
            _stream_id: u64,
        ) -> lumen_core::error::Result<(Vec<u8>, bool)> {
            match self.read_stream_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok((Vec::new(), true)),
            }
        }
        fn webtransport_poll_incoming_uni_streams(&self, _handle: i32) -> lumen_core::error::Result<Vec<u64>> {
            match self.poll_incoming_uni_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok(Vec::new()),
            }
        }
        fn webtransport_read_incoming_uni_stream(
            &self,
            _handle: i32,
            _stream_id: u64,
        ) -> lumen_core::error::Result<(Vec<u8>, bool)> {
            match self.read_incoming_uni_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok((Vec::new(), true)),
            }
        }
        fn webtransport_poll_incoming_bidi_streams(&self, _handle: i32) -> lumen_core::error::Result<Vec<u64>> {
            match self.poll_incoming_bidi_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok(Vec::new()),
            }
        }
        fn webtransport_read_incoming_bidi_stream(
            &self,
            _handle: i32,
            _stream_id: u64,
        ) -> lumen_core::error::Result<(Vec<u8>, bool)> {
            match self.read_incoming_bidi_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok((Vec::new(), true)),
            }
        }
        fn webtransport_send_datagram(&self, handle: i32, data: &[u8]) -> lumen_core::error::Result<()> {
            *self.last_send_datagram.lock().unwrap() = Some((handle, data.to_vec()));
            match &self.send_datagram_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_poll_incoming_datagrams(&self, _handle: i32) -> lumen_core::error::Result<Vec<Vec<u8>>> {
            match self.poll_incoming_datagrams_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok(Vec::new()),
            }
        }
        fn webtransport_close_session(
            &self,
            handle: i32,
            close_code: u32,
            reason: &str,
        ) -> lumen_core::error::Result<()> {
            *self.last_close_session.lock().unwrap() = Some((handle, close_code, reason.to_string()));
            match &self.close_session_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_poll_closed(
            &self,
            _handle: i32,
        ) -> lumen_core::error::Result<lumen_core::ext::WebTransportSessionState> {
            match self.poll_closed_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok(lumen_core::ext::WebTransportSessionState::Open),
            }
        }
    }

    /// Unlike [`rt_with_webtransport`], does not call `install_webtransport_v8`
    /// a second time — `install_dom` already installs it once, internally, via
    /// the `fetch_provider` argument here; a second `register_native` call for
    /// the same name would not be reliably observable as an override, so the
    /// provider has to go in through this one call.
    fn rt_with_webtransport_provider(
        result: lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>,
    ) -> V8JsRuntime {
        rt_with_webtransport_provider_and_uni_result(result, Ok(7))
    }

    /// Like [`rt_with_webtransport_provider`], but also controls what
    /// `webtransport_open_uni_stream` answers — GAP-WEBTRANSPORT срез 3b.
    fn rt_with_webtransport_provider_and_uni_result(
        result: lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>,
        uni_stream_result: lumen_core::error::Result<u64>,
    ) -> V8JsRuntime {
        rt_with_webtransport_provider_full(result, uni_stream_result, Ok(())).0
    }

    /// Like [`rt_with_webtransport_provider_and_uni_result`], but also
    /// controls what `webtransport_write_uni_stream` answers and returns the
    /// `Arc<Mutex<...>>` the test can inspect for the last write's
    /// `(handle, stream_id, bytes)` — GAP-WEBTRANSPORT срез 3c.
    fn rt_with_webtransport_provider_full(
        result: lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>,
        uni_stream_result: lumen_core::error::Result<u64>,
        write_stream_result: lumen_core::error::Result<()>,
    ) -> (V8JsRuntime, Arc<StubFetch>) {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        let stub = Arc::new(StubFetch {
            result: std::sync::Mutex::new(Some(result)),
            uni_stream_result,
            bidi_stream_result: Ok(9),
            write_stream_result,
            last_write: std::sync::Mutex::new(None),
            close_stream_result: Ok(()),
            abort_stream_result: Ok(()),
            last_close: std::sync::Mutex::new(None),
            last_abort: std::sync::Mutex::new(None),
            read_stream_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            send_datagram_result: Ok(()),
            last_send_datagram: std::sync::Mutex::new(None),
            poll_incoming_datagrams_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            close_session_result: Ok(()),
            last_close_session: std::sync::Mutex::new(None),
            poll_closed_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
        });
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = stub.clone();
        rt.install_dom(doc, "", Some(provider), None, None, None, None, None, None, None, None, false)
            .unwrap();
        (rt, stub)
    }

    fn check(rt: &V8JsRuntime, expr: &str) {
        assert_eq!(rt.eval(expr).unwrap(), JsValue::Bool(true), "assertion failed for `{expr}`");
    }

    #[test]
    fn constructor_rejects_non_https_scheme_synchronously() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                try { new WebTransport('http://example.com/wt'); return false; } \
                catch (e) { return e instanceof DOMException && e.name === 'SyntaxError'; } \
            })()",
        );
    }

    #[test]
    fn constructor_rejects_fragment() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                try { new WebTransport('https://example.com/wt#frag'); return false; } \
                catch (e) { return e instanceof DOMException && e.name === 'SyntaxError'; } \
            })()",
        );
    }

    #[test]
    fn constructor_accepts_https_and_exposes_spec_shape() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var wt = new WebTransport('https://example.com:4433/wt'); \
                return wt.ready instanceof Promise && \
                    wt.closed instanceof Promise && \
                    wt.datagrams instanceof WebTransportDatagramDuplexStream && \
                    wt.datagrams.readable instanceof ReadableStream && \
                    wt.datagrams.writable instanceof WritableStream && \
                    wt.incomingBidirectionalStreams instanceof ReadableStream && \
                    wt.incomingUnidirectionalStreams instanceof ReadableStream && \
                    typeof wt.createBidirectionalStream === 'function' && \
                    typeof wt.createUnidirectionalStream === 'function' && \
                    typeof wt.close === 'function'; \
            })()",
        );
    }

    /// `createBidirectionalStream()` rejects synchronously (no `setTimeout`
    /// involved, unlike `ready`/`closed`), so the microtask checkpoint V8
    /// runs at the end of the first `eval` already resolved the `.catch`
    /// handler by the time the second `eval` reads its result back.
    #[test]
    fn create_streams_reject_with_web_transport_error_before_ready() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtTestResult = false; \
            (function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.createBidirectionalStream().catch(function(e) { \
                    globalThis._wtTestResult = (e instanceof WebTransportError) && e.source === 'session'; \
                }); \
            })();",
        )
        .unwrap();
        check(&rt, "_wtTestResult");
    }

    /// GAP-WEBTRANSPORT срез 3b: same shape as
    /// `create_streams_reject_with_web_transport_error_before_ready` for the
    /// other stream constructor — `createUnidirectionalStream()` now takes a
    /// different code path (checks `self._handle` instead of unconditionally
    /// rejecting), so it needs its own coverage that the no-handle-yet case
    /// still rejects synchronously with the same `WebTransportError` shape.
    #[test]
    fn create_unidirectional_stream_rejects_before_ready() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtTestResult = false; \
            (function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.createUnidirectionalStream().catch(function(e) { \
                    globalThis._wtTestResult = (e instanceof WebTransportError) && e.source === 'session'; \
                }); \
            })();",
        )
        .unwrap();
        check(&rt, "_wtTestResult");
    }

    /// GAP-WEBTRANSPORT срез 2b: no `setTimeout` involved here — the native
    /// binding itself is called directly, same workaround
    /// `websocket_connect_fail_fires_onerror` uses ("we can't pump the
    /// timeout in this test"), to check the JSON shape `ready`'s callback
    /// parses without depending on macrotask pumping this harness lacks.
    #[test]
    fn native_open_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_reports_ok_and_status_on_success() {
        let rt = rt_with_webtransport_provider(Ok(lumen_core::ext::JsWebTransportSession {
            handle: 0,
            status: 200,
        }));
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains("200"), "expected status 200, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 3b: the JSON `_lumen_webtransport_open` reports
    /// on success now also carries the session `handle`, so
    /// `createUnidirectionalStream()` can address it later.
    #[test]
    fn native_open_reports_handle_on_success() {
        let rt = rt_with_webtransport_provider(Ok(lumen_core::ext::JsWebTransportSession {
            handle: 42,
            status: 200,
        }));
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""handle":42"#), "expected handle 42, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 3b: direct native-call coverage for
    /// `_lumen_webtransport_open_uni_stream`, same "no `setTimeout` pumping
    /// in this harness" workaround as the `_lumen_webtransport_open` tests
    /// above.
    #[test]
    fn native_open_uni_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_open_uni_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_uni_stream_reports_stream_id_on_success() {
        let rt = rt_with_webtransport_provider_and_uni_result(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(6),
        );
        let r = rt.eval("_lumen_webtransport_open_uni_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamId":6"#), "expected streamId 6, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_uni_stream_reports_provider_error_message() {
        let rt = rt_with_webtransport_provider_and_uni_result(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Err(lumen_core::error::Error::Network("WebTransport session not found".to_string())),
        );
        let r = rt.eval("_lumen_webtransport_open_uni_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
                assert!(s.contains("session not found"), "expected the message, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 3c: direct native-call coverage for
    /// `_lumen_webtransport_write_stream`, same "no `setTimeout` pumping in
    /// this harness" workaround as the other native-call tests above.
    #[test]
    fn native_write_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_write_stream(0, 2, new Uint8Array([1]))").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_write_stream_forwards_handle_stream_id_and_bytes() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 5, status: 200 }),
            Ok(2),
            Ok(()),
        );
        let r = rt.eval("_lumen_webtransport_write_stream(5, 2, new Uint8Array([104, 105]))").unwrap();
        match r {
            lumen_core::JsValue::String(s) => assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}"),
            other => panic!("expected a String, got {other:?}"),
        }
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (5, 2, vec![104, 105]));
    }

    #[test]
    fn native_write_stream_reports_provider_error_message() {
        let (rt, _stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(2),
            Err(lumen_core::error::Error::Network("WebTransport session not found".to_string())),
        );
        let r = rt.eval("_lumen_webtransport_write_stream(0, 2, new Uint8Array([1]))").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
                assert!(s.contains("session not found"), "expected the message, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `createUnidirectionalStream()` resolves a real
    /// `WritableStream` whose `write()` reaches
    /// `_lumen_webtransport_write_stream` with the session's handle and the
    /// stream id `createUnidirectionalStream()` itself opened — proves the
    /// two natives compose through the shim, not just each in isolation.
    #[test]
    fn create_unidirectional_stream_write_reaches_the_native_with_the_right_ids() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtWriteOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                var writer = stream.getWriter(); \
                return writer.write(new Uint8Array([1, 2, 3])); \
            }).then(function() { \
                globalThis._wtWriteOk = true; \
            });",
        )
        .unwrap();
        // `ready` resolves from a `setTimeout(0)` callback (the constructor's
        // native call, срез 3b) — this harness needs an explicit timer pump
        // (`_lumen_tick_timers`, see `internal_globals.rs`'s
        // `engine_state_stays_writable`) to run it before the promise chain
        // above can proceed.
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtWriteOk");
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (3, 2, vec![1, 2, 3]));
    }

    #[test]
    fn create_unidirectional_stream_write_rejects_on_provider_error() {
        let (rt, _stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(2),
            Err(lumen_core::error::Error::Network("boom".to_string())),
        );
        rt.eval(
            "globalThis._wtWriteRejected = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                var writer = stream.getWriter(); \
                return writer.write(new Uint8Array([1])); \
            }).catch(function(e) { \
                globalThis._wtWriteRejected = (e instanceof WebTransportError) && e.source === 'stream'; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtWriteRejected");
    }

    /// GAP-WEBTRANSPORT срез 3d: direct native-call coverage for
    /// `_lumen_webtransport_close_stream`, same "no `setTimeout` pumping in
    /// this harness" workaround as the other native-call tests above.
    #[test]
    fn native_close_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_close_stream(0, 2)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_abort_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_abort_stream(0, 2, 5)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `createUnidirectionalStream()` resolves a `WritableStream`
    /// whose `getWriter().close()` reaches `_lumen_webtransport_close_stream`
    /// with the session's handle and the stream id
    /// `createUnidirectionalStream()` itself opened.
    #[test]
    fn create_unidirectional_stream_close_reaches_the_native_with_the_right_ids() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtCloseOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().close(); \
            }).then(function() { \
                globalThis._wtCloseOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtCloseOk");
        let last = *stub.last_close.lock().unwrap();
        assert_eq!(last, Some((3, 2)));
    }

    #[test]
    fn create_unidirectional_stream_close_rejects_on_provider_error() {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        let stub = Arc::new(StubFetch {
            result: std::sync::Mutex::new(Some(Ok(lumen_core::ext::JsWebTransportSession {
                handle: 0,
                status: 200,
            }))),
            uni_stream_result: Ok(2),
            bidi_stream_result: Ok(9),
            write_stream_result: Ok(()),
            last_write: std::sync::Mutex::new(None),
            close_stream_result: Err(lumen_core::error::Error::Network("boom".to_string())),
            abort_stream_result: Ok(()),
            last_close: std::sync::Mutex::new(None),
            last_abort: std::sync::Mutex::new(None),
            read_stream_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            send_datagram_result: Ok(()),
            last_send_datagram: std::sync::Mutex::new(None),
            poll_incoming_datagrams_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            close_session_result: Ok(()),
            last_close_session: std::sync::Mutex::new(None),
            poll_closed_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
        });
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = stub.clone();
        rt.install_dom(doc, "", Some(provider), None, None, None, None, None, None, None, None, false)
            .unwrap();
        rt.eval(
            "globalThis._wtCloseRejected = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().close(); \
            }).catch(function(e) { \
                globalThis._wtCloseRejected = (e instanceof WebTransportError) && e.source === 'stream'; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtCloseRejected");
    }

    /// End-to-end: `createUnidirectionalStream()` resolves a `WritableStream`
    /// whose `getWriter().abort(reason)` reaches
    /// `_lumen_webtransport_abort_stream` with the session's handle, the
    /// stream id, and the `reason`'s `streamErrorCode` when `reason` is a
    /// `WebTransportError`.
    #[test]
    fn create_unidirectional_stream_abort_forwards_the_stream_error_code() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtAbortOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().abort(new WebTransportError({ source: 'stream', streamErrorCode: 9 })); \
            }).then(function() { \
                globalThis._wtAbortOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtAbortOk");
        let last = *stub.last_abort.lock().unwrap();
        assert_eq!(last, Some((3, 2, 9)));
    }

    #[test]
    fn create_unidirectional_stream_abort_defaults_to_zero_for_a_plain_reason() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtAbortOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().abort('some reason'); \
            }).then(function() { \
                globalThis._wtAbortOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtAbortOk");
        let last = *stub.last_abort.lock().unwrap();
        assert_eq!(last, Some((3, 2, 0)));
    }

    /// GAP-WEBTRANSPORT срез 4b: direct native-call coverage for
    /// `_lumen_webtransport_open_bidi_stream`, same "no `setTimeout` pumping
    /// in this harness" workaround as the uni-stream native-call tests.
    #[test]
    fn native_open_bidi_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_open_bidi_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_bidi_stream_reports_stream_id_on_success() {
        let (rt, _stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(2),
            Ok(()),
        );
        let r = rt.eval("_lumen_webtransport_open_bidi_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamId":9"#), "expected streamId 9, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// `createBidirectionalStream()` rejects synchronously with the same
    /// "no live handle yet" shape as `createUnidirectionalStream()`
    /// (`create_unidirectional_stream_rejects_before_ready`).
    #[test]
    fn create_bidirectional_stream_rejects_before_ready() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtTestResult = false; \
            (function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.createBidirectionalStream().catch(function(e) { \
                    globalThis._wtTestResult = (e instanceof WebTransportError) && e.source === 'session'; \
                }); \
            })();",
        )
        .unwrap();
        check(&rt, "_wtTestResult");
    }

    /// End-to-end: `createBidirectionalStream()` resolves a real
    /// `WebTransportBidirectionalStream` whose `writable.write()` reaches
    /// `_lumen_webtransport_write_stream` with the session's handle and the
    /// stream id `createBidirectionalStream()` itself opened — same
    /// composition proof as `create_unidirectional_stream_write_reaches_the_native_with_the_right_ids`,
    /// for the bidi transport primitive (срез 4a/4b).
    #[test]
    fn create_bidirectional_stream_write_reaches_the_native_with_the_right_ids() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtWriteOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createBidirectionalStream(); \
            }).then(function(stream) { \
                return (stream instanceof WebTransportBidirectionalStream) && \
                    stream.readable instanceof ReadableStream ? \
                    stream.writable.getWriter().write(new Uint8Array([1, 2, 3])) : \
                    Promise.reject(new Error('not a WebTransportBidirectionalStream')); \
            }).then(function() { \
                globalThis._wtWriteOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtWriteOk");
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (3, 9, vec![1, 2, 3]));
    }

    /// GAP-WEBTRANSPORT срез 4c: direct native-call coverage for
    /// `_lumen_webtransport_read_stream`, same "no provider → unsupported"
    /// shape as every other WebTransport native.
    #[test]
    fn native_read_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_read_stream(0, 0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 4d: direct native-call coverage for
    /// `_lumen_webtransport_poll_incoming_uni_streams`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_poll_incoming_uni_streams_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_poll_incoming_uni_streams(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_poll_incoming_uni_streams_reports_stream_ids_on_success() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_uni_results.lock().unwrap().push_back(Ok(vec![3, 7]));
        let r = rt.eval("_lumen_webtransport_poll_incoming_uni_streams(3)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamIds":[3,7]"#), "expected streamIds [3,7], got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 4d: direct native-call coverage for
    /// `_lumen_webtransport_read_incoming_uni_stream`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_read_incoming_uni_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_read_incoming_uni_stream(0, 3)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `incomingUnidirectionalStreams` yields a nested
    /// `ReadableStream` once `_lumen_webtransport_poll_incoming_uni_streams`
    /// reports a discovered id, and that nested stream's bytes come from
    /// `_lumen_webtransport_read_incoming_uni_stream` — proves the discovery
    /// loop and the per-stream read loop compose, not just each in
    /// isolation.
    #[test]
    fn incoming_unidirectional_streams_yields_a_readable_then_bytes() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_uni_results.lock().unwrap().push_back(Ok(vec![9]));
        {
            let mut reads = stub.read_incoming_uni_results.lock().unwrap();
            reads.push_back(Ok((vec![42u8, 43, 44], false)));
            reads.push_back(Ok((Vec::new(), true)));
        }
        rt.eval(
            "globalThis._wtIncomingLen = -1; globalThis._wtIncomingDone = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                var reader = globalThis._wt.incomingUnidirectionalStreams.getReader(); \
                return reader.read(); \
            }).then(function(res) { \
                var innerReader = res.value.getReader(); \
                return innerReader.read().then(function(res1) { \
                    globalThis._wtIncomingLen = res1.value ? res1.value.length : -1; \
                    return innerReader.read(); \
                }).then(function(res2) { \
                    globalThis._wtIncomingDone = res2.done === true; \
                }); \
            });",
        )
        .unwrap();
        // Unlike `createBidirectionalStream()`'s readable (one tick: the
        // stream already exists by the time `.then()` reads it), this chain
        // has one more asynchronous hop — the session's `ready` resolution
        // and `incomingUnidirectionalStreams`'s first `pull()` do not
        // necessarily settle within the same tick's microtask cascade — so
        // it needs a few more calls, same as production driving this via
        // the shell's per-frame `_lumen_tick_timers()`.
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(&rt, "_wtIncomingLen === 3");
        check(&rt, "_wtIncomingDone");
    }

    /// GAP-WEBTRANSPORT срез 4e: direct native-call coverage for
    /// `_lumen_webtransport_poll_incoming_bidi_streams`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_poll_incoming_bidi_streams_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_poll_incoming_bidi_streams(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_poll_incoming_bidi_streams_reports_stream_ids_on_success() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_bidi_results.lock().unwrap().push_back(Ok(vec![5, 13]));
        let r = rt.eval("_lumen_webtransport_poll_incoming_bidi_streams(3)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamIds":[5,13]"#), "expected streamIds [5,13], got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 4e: direct native-call coverage for
    /// `_lumen_webtransport_read_incoming_bidi_stream`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_read_incoming_bidi_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_read_incoming_bidi_stream(0, 5)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `incomingBidirectionalStreams` yields a full
    /// `WebTransportBidirectionalStream` once
    /// `_lumen_webtransport_poll_incoming_bidi_streams` reports a discovered
    /// id — its `readable` pulls bytes through
    /// `_lumen_webtransport_read_incoming_bidi_stream` (same discovery+read
    /// composition proof as `incoming_unidirectional_streams_yields_a_readable_then_bytes`)
    /// and its `writable` reaches the ordinary `_lumen_webtransport_write_stream`
    /// unchanged — proving the send half the Rust side pre-registers on
    /// discovery is actually usable from JS, not just present on the Rust
    /// side.
    #[test]
    fn incoming_bidirectional_streams_yields_a_stream_readable_and_writable() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_bidi_results.lock().unwrap().push_back(Ok(vec![13]));
        {
            let mut reads = stub.read_incoming_bidi_results.lock().unwrap();
            reads.push_back(Ok((vec![7u8, 8, 9], false)));
            reads.push_back(Ok((Vec::new(), true)));
        }
        rt.eval(
            "globalThis._wtIncomingLen = -1; globalThis._wtIncomingDone = false; \
            globalThis._wtIncomingWriteOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                var reader = globalThis._wt.incomingBidirectionalStreams.getReader(); \
                return reader.read(); \
            }).then(function(res) { \
                var stream = res.value; \
                var writePromise = (stream instanceof WebTransportBidirectionalStream) ? \
                    stream.writable.getWriter().write(new Uint8Array([1, 2, 3])) : \
                    Promise.reject(new Error('not a WebTransportBidirectionalStream')); \
                var innerReader = stream.readable.getReader(); \
                var readPromise = innerReader.read().then(function(res1) { \
                    globalThis._wtIncomingLen = res1.value ? res1.value.length : -1; \
                    return innerReader.read(); \
                }).then(function(res2) { \
                    globalThis._wtIncomingDone = res2.done === true; \
                }); \
                return Promise.all([writePromise, readPromise]); \
            }).then(function() { \
                globalThis._wtIncomingWriteOk = true; \
            });",
        )
        .unwrap();
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(&rt, "_wtIncomingLen === 3");
        check(&rt, "_wtIncomingDone");
        check(&rt, "_wtIncomingWriteOk");
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (3, 13, vec![1, 2, 3]));
    }

    /// End-to-end: `createBidirectionalStream()`'s `readable` polls
    /// `_lumen_webtransport_read_stream` — a first poll answering bytes with
    /// `finished:false` yields one chunk, and a second answering
    /// `finished:true` with no bytes closes the stream (`reader.read()`'s
    /// `done`), without a `setTimeout` retry needed for either since
    /// `pull()` calls the native synchronously before ever queuing a timer.
    #[test]
    fn create_bidirectional_stream_readable_yields_bytes_then_closes() {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        let mut reads = std::collections::VecDeque::new();
        reads.push_back(Ok((vec![1u8, 2, 3], false)));
        reads.push_back(Ok((Vec::new(), true)));
        let stub = Arc::new(StubFetch {
            result: std::sync::Mutex::new(Some(Ok(lumen_core::ext::JsWebTransportSession {
                handle: 3,
                status: 200,
            }))),
            uni_stream_result: Ok(2),
            bidi_stream_result: Ok(9),
            write_stream_result: Ok(()),
            last_write: std::sync::Mutex::new(None),
            close_stream_result: Ok(()),
            abort_stream_result: Ok(()),
            last_close: std::sync::Mutex::new(None),
            last_abort: std::sync::Mutex::new(None),
            read_stream_results: std::sync::Mutex::new(reads),
            poll_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            send_datagram_result: Ok(()),
            last_send_datagram: std::sync::Mutex::new(None),
            poll_incoming_datagrams_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            close_session_result: Ok(()),
            last_close_session: std::sync::Mutex::new(None),
            poll_closed_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
        });
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = stub.clone();
        rt.install_dom(doc, "", Some(provider), None, None, None, None, None, None, None, None, false)
            .unwrap();
        rt.eval(
            "globalThis._wtFirstLen = -1; globalThis._wtDone = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createBidirectionalStream(); \
            }).then(function(stream) { \
                var reader = stream.readable.getReader(); \
                return reader.read().then(function(res1) { \
                    globalThis._wtFirstLen = res1.value ? res1.value.length : -1; \
                    return reader.read(); \
                }).then(function(res2) { \
                    globalThis._wtDone = res2.done === true; \
                }); \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtFirstLen === 3");
        check(&rt, "_wtDone");
    }

    #[test]
    fn native_open_reports_provider_error_message() {
        let rt = rt_with_webtransport_provider(Err(lumen_core::error::Error::Network(
            "WebTransport Extended CONNECT rejected: status 403".to_string(),
        )));
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
                assert!(s.contains("403"), "expected the status in the message, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn web_transport_error_source_defaults_to_session() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var e = new WebTransportError({ message: 'boom' }); \
                return e instanceof DOMException && \
                    e.name === 'WebTransportError' && \
                    e.message === 'boom' && \
                    e.source === 'session' && \
                    e.streamErrorCode === null; \
            })()",
        );
    }

    #[test]
    fn web_transport_error_stream_source_and_code_roundtrip() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var e = new WebTransportError({ source: 'stream', streamErrorCode: 7 }); \
                return e.source === 'stream' && e.streamErrorCode === 7; \
            })()",
        );
    }

    #[test]
    fn close_is_idempotent() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.close(); \
                wt.close({ closeCode: 0, reason: '' }); \
                return true; \
            })()",
        );
    }

    // ---- datagrams (GAP-WEBTRANSPORT срез datagrams-b) -------------------

    /// GAP-WEBTRANSPORT срез datagrams-b: direct native-call coverage for
    /// `_lumen_webtransport_send_datagram`, same "no provider → unsupported"
    /// shape as every other WebTransport native.
    #[test]
    fn native_send_datagram_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_send_datagram(0, [1, 2, 3])").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез datagrams-b: direct native-call coverage for
    /// `_lumen_webtransport_poll_incoming_datagrams`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_poll_incoming_datagrams_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_poll_incoming_datagrams(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_poll_incoming_datagrams_reports_nested_byte_arrays_on_success() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_datagrams_results
            .lock()
            .unwrap()
            .push_back(Ok(vec![vec![1, 2, 3], vec![4, 5]]));
        let r = rt.eval("_lumen_webtransport_poll_incoming_datagrams(3)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(
                    s.contains(r#""datagrams":[[1,2,3],[4,5]]"#),
                    "expected nested datagram arrays, got {s}"
                );
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// `datagrams.writable` rejects synchronously (no live handle yet, same
    /// "no `setTimeout` involved" shape as `createBidirectionalStream()`'s
    /// pre-`ready` rejection) rather than throwing — the `WritableStream`
    /// itself is constructed eagerly, unlike `createUnidirectionalStream()`'s
    /// promise-returning call.
    #[test]
    fn datagrams_writable_rejects_before_ready() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtTestResult = false; \
            (function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.datagrams.writable.getWriter().write(new Uint8Array([1])).catch(function(e) { \
                    globalThis._wtTestResult = (e instanceof WebTransportError) && e.source === 'session'; \
                }); \
            })();",
        )
        .unwrap();
        check(&rt, "_wtTestResult");
    }

    /// End-to-end: `datagrams.writable.getWriter().write()` reaches
    /// `_lumen_webtransport_send_datagram` with the session's handle and the
    /// written bytes — same composition proof as
    /// `create_bidirectional_stream_write_reaches_the_native_with_the_right_ids`,
    /// for the datagram transport primitive.
    #[test]
    fn write_datagram_reaches_the_native_with_the_right_handle_and_bytes() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtDatagramWriteOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.datagrams.writable.getWriter().write(new Uint8Array([9, 8, 7])); \
            }).then(function() { \
                globalThis._wtDatagramWriteOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtDatagramWriteOk");
        let last = stub.last_send_datagram.lock().unwrap().clone().expect("datagram send recorded");
        assert_eq!(last, (3, vec![9, 8, 7]));
    }

    /// End-to-end: `datagrams.readable` yields a `Uint8Array` chunk straight
    /// from `_lumen_webtransport_poll_incoming_datagrams`'s answer — unlike
    /// `incomingUnidirectionalStreams`, no nested stream is involved (a
    /// datagram is a whole chunk, not a byte-oriented sub-stream), so this is
    /// the direct counterpart of `incoming_unidirectional_streams_yields_a_readable_then_bytes`
    /// one hop shorter.
    #[test]
    fn datagrams_readable_yields_bytes_from_the_native_poll() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_datagrams_results.lock().unwrap().push_back(Ok(vec![vec![1, 2, 3]]));
        rt.eval(
            "globalThis._wtDatagramLen = -1; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                var reader = globalThis._wt.datagrams.readable.getReader(); \
                return reader.read(); \
            }).then(function(res) { \
                globalThis._wtDatagramLen = res.value ? res.value.length : -1; \
            });",
        )
        .unwrap();
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(&rt, "_wtDatagramLen === 3");
    }

    /// `datagrams.readable`'s poll loop stops for good once the session
    /// closes — same "controller.close() on `_closed`/`_readyFailed`" gate
    /// `openIncomingUnidirectionalStreams`/`openIncomingBidirectionalStreams`
    /// already share, proven here via `close()` rather than a failed `ready`.
    #[test]
    fn datagrams_readable_closes_when_the_session_closes() {
        let (rt, _stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtDatagramDone = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                globalThis._wt.close(); \
                var reader = globalThis._wt.datagrams.readable.getReader(); \
                return reader.read(); \
            }).then(function(res) { \
                globalThis._wtDatagramDone = res.done === true; \
            });",
        )
        .unwrap();
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(&rt, "_wtDatagramDone");
    }

    /// GAP-WEBTRANSPORT срез 5: direct native-call coverage for
    /// `_lumen_webtransport_close_session`, same "no provider → unsupported"
    /// shape as `native_close_stream_reports_unsupported_with_no_provider`.
    #[test]
    fn native_close_session_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_close_session(0, 7, 'bye')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `close({closeCode, reason})` after `ready` reaches
    /// `_lumen_webtransport_close_session` with the session's handle and the
    /// caller's own `closeCode`/`reason`, and fulfills `closed` with that same
    /// pair — the primary lifecycle contract срез 5 adds (spec §5.4: a
    /// locally initiated close always fulfills `closed`, it never rejects).
    #[test]
    fn close_reaches_the_native_and_fulfills_closed_with_the_same_close_info() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtCloseInfo = null; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                globalThis._wt.close({ closeCode: 42, reason: 'bye' }); \
                return globalThis._wt.closed; \
            }).then(function(info) { \
                globalThis._wtCloseInfo = info; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtCloseInfo !== null && _wtCloseInfo.closeCode === 42 && _wtCloseInfo.reason === 'bye'");
        let last = stub.last_close_session.lock().unwrap().clone().expect("close was recorded");
        assert_eq!(last, (3, 42, "bye".to_string()));
    }

    /// `close()` with no `closeInfo` at all defaults to `{closeCode: 0, reason: ''}`
    /// (spec §5.4's WebIDL dictionary defaults), not a thrown error or a
    /// `null`/`undefined` reaching the native call.
    #[test]
    fn close_with_no_argument_defaults_close_code_and_reason() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { globalThis._wt.close(); });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        let last = stub.last_close_session.lock().unwrap().clone().expect("close was recorded");
        assert_eq!(last, (3, 0, String::new()));
    }

    /// `close({reason})` with a `reason` longer than 1024 UTF-8 bytes throws a
    /// `TypeError` synchronously (spec §5.4) rather than truncating or
    /// rejecting `closed`.
    #[test]
    fn close_rejects_a_reason_over_1024_utf8_bytes() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                var longReason = new Array(1026).join('a'); \
                try { wt.close({ reason: longReason }); return false; } \
                catch (e) { return e instanceof TypeError; } \
            })()",
        );
    }

    /// Closing before `ready` ever settles still fulfills `closed` with the
    /// caller's own `closeInfo` — closing does not wait for the session to
    /// finish connecting, and a session that never got a handle at all is not
    /// the same as one whose `ready` explicitly failed (`_readyFailed`, tested
    /// separately by every `openIncoming*`/`datagrams` "stops on close" test).
    #[test]
    fn close_before_ready_settles_still_fulfills_closed() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtCloseInfo = null; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.close({ closeCode: 5, reason: 'early' }); \
            globalThis._wt.closed.then(function(info) { \
                globalThis._wtCloseInfo = info; \
            });",
        )
        .unwrap();
        check(&rt, "_wtCloseInfo !== null && _wtCloseInfo.closeCode === 5 && _wtCloseInfo.reason === 'early'");
    }

    /// A failed `ready` (no `fetch_provider` at all, so the native always
    /// answers "not supported") must keep rejecting `closed` — `close()`
    /// called afterward must not paper over that with a fulfillment.
    #[test]
    fn close_after_ready_failed_does_not_override_the_rejection() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtClosedRejected = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.catch(function() {}); \
            globalThis._wt.closed.catch(function(e) { \
                globalThis._wtClosedRejected = e instanceof WebTransportError; \
                globalThis._wt.close(); \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtClosedRejected");
    }

    /// GAP-WEBTRANSPORT, remaining sub-slice of срез 5: direct native-call
    /// coverage for `_lumen_webtransport_poll_closed`, same "no provider →
    /// unsupported" shape as `native_close_session_reports_unsupported_with_no_provider`.
    #[test]
    fn native_poll_closed_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_poll_closed(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: once `webtransport_poll_closed` reports `closedByPeer`,
    /// `pollSessionClosedByPeer` fulfills `closed` with that same
    /// `closeCode`/`reason` — a peer-initiated close settles `closed` the
    /// same way a locally initiated one does (spec §5.4), the difference
    /// being which side supplied the `close_code`/`reason`.
    #[test]
    fn peer_close_fulfills_closed_with_the_peers_close_info() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_closed_results.lock().unwrap().push_back(Ok(
            lumen_core::ext::WebTransportSessionState::ClosedByPeer {
                close_code: 99,
                reason: "server done".to_string(),
            },
        ));
        rt.eval(
            "globalThis._wtCloseInfo = null; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.closed.then(function(info) { \
                globalThis._wtCloseInfo = info; \
            });",
        )
        .unwrap();
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(
            &rt,
            "_wtCloseInfo !== null && _wtCloseInfo.closeCode === 99 && _wtCloseInfo.reason === 'server done'",
        );
    }

    /// End-to-end: once `webtransport_poll_closed` reports `connectionLost`,
    /// `pollSessionClosedByPeer` rejects `closed` with a `WebTransportError` —
    /// a fatal connection error the client did not initiate has no
    /// `close_code`/`reason` to report, unlike a local or peer-initiated
    /// close (spec §5.4).
    #[test]
    fn connection_lost_rejects_closed_with_a_webtransport_error() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_closed_results
            .lock()
            .unwrap()
            .push_back(Ok(lumen_core::ext::WebTransportSessionState::ConnectionLost));
        rt.eval(
            "globalThis._wtClosedRejected = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.closed.catch(function(e) { \
                globalThis._wtClosedRejected = e instanceof WebTransportError; \
            });",
        )
        .unwrap();
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(&rt, "_wtClosedRejected");
    }

    /// A local `close()` racing the peer-close poll loop wins: the loop's
    /// `session._closed` guard stops it from ever calling
    /// `_lumen_webtransport_poll_closed` again once `close()` ran, so
    /// `closed` settles with the caller's own `closeInfo`, not whatever the
    /// (never-checked) queued peer-close answer would have said.
    #[test]
    fn local_close_wins_the_race_against_the_peer_close_poll_loop() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_closed_results.lock().unwrap().push_back(Ok(
            lumen_core::ext::WebTransportSessionState::ClosedByPeer {
                close_code: 1,
                reason: "should never be observed".to_string(),
            },
        ));
        rt.eval(
            "globalThis._wtCloseInfo = null; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                globalThis._wt.close({ closeCode: 7, reason: 'local' }); \
            }); \
            globalThis._wt.closed.then(function(info) { \
                globalThis._wtCloseInfo = info; \
            });",
        )
        .unwrap();
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(
            &rt,
            "_wtCloseInfo !== null && _wtCloseInfo.closeCode === 7 && _wtCloseInfo.reason === 'local'",
        );
    }
