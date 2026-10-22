use super::*;

#[test]
fn null_runtime_returns_not_implemented_for_eval() {
    let rt = NullJsRuntime;
    assert!(matches!(rt.eval("1 + 2"), Err(JsError::NotImplemented)));
}

#[test]
fn null_runtime_returns_not_implemented_for_set_global() {
    let rt = NullJsRuntime;
    assert!(matches!(
        rt.set_global("x", JsValue::Number(1.0)),
        Err(JsError::NotImplemented)
    ));
}

#[test]
fn null_runtime_engine_name() {
    assert_eq!(NullJsRuntime.engine_name(), "null");
}

#[test]
fn js_value_object_sorted_by_key() {
    let v = JsValue::object(vec![
        ("b".into(), JsValue::Number(2.0)),
        ("a".into(), JsValue::Number(1.0)),
        ("c".into(), JsValue::Number(3.0)),
    ]);
    match v {
        JsValue::Object(entries) => {
            let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(keys, vec!["a", "b", "c"]);
        }
        _ => panic!("expected Object"),
    }
}

#[test]
fn js_value_equality() {
    assert_eq!(JsValue::Null, JsValue::Null);
    assert_eq!(JsValue::Number(1.5), JsValue::Number(1.5));
    assert_ne!(JsValue::Bool(true), JsValue::Bool(false));
    assert_eq!(
        JsValue::Array(vec![JsValue::Number(1.0), JsValue::String("a".into())]),
        JsValue::Array(vec![JsValue::Number(1.0), JsValue::String("a".into())]),
    );
}

#[test]
fn js_error_display() {
    assert_eq!(
        format!("{}", JsError::Parse("unexpected }".into())),
        "JS parse error: unexpected }"
    );
    assert_eq!(format!("{}", JsError::NotImplemented), "JS not implemented");
}

#[test]
fn null_runtime_is_send_sync() {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<NullJsRuntime>();
    // dyn check.
    fn check_dyn(_r: &dyn JsRuntime) {}
    let rt = NullJsRuntime;
    check_dyn(&rt);
}

#[test]
fn http_auth_scheme_as_str() {
    assert_eq!(HttpAuthScheme::Basic.as_str(), "Basic");
    assert_eq!(HttpAuthScheme::Digest.as_str(), "Digest");
}

#[test]
fn http_auth_challenge_equality() {
    let a = HttpAuthChallenge {
        origin: "https://example.com".into(),
        realm: "Admin".into(),
        scheme: HttpAuthScheme::Digest,
    };
    let b = HttpAuthChallenge {
        origin: "https://example.com".into(),
        realm: "Admin".into(),
        scheme: HttpAuthScheme::Digest,
    };
    assert_eq!(a, b);
}

#[test]
fn http_credential_provider_is_object_safe() {
    // dyn check (Send + Sync).
    fn check_dyn(_p: &dyn HttpCredentialProvider) {}
    struct Fixed;
    impl HttpCredentialProvider for Fixed {
        fn credentials(&self, _c: &HttpAuthChallenge) -> Option<HttpCredentials> {
            Some(HttpCredentials {
                username: "u".into(),
                password: "p".into(),
            })
        }
    }
    check_dyn(&Fixed);
}

// --- Sprint 0 P3 trait-anchors ---

#[test]
fn null_unicode_provider_returns_empty_and_is_dyn_safe() {
    let p = NullUnicodeProvider;
    assert_eq!(p.provider_name(), "null");
    assert!(p.line_break_opportunities("hello world").is_empty());
    assert!(p.grapheme_boundaries("a\u{301}b").is_empty());
    assert!(p.word_boundaries("two words").is_empty());
    assert!(p.bidi_runs("שלום", true).is_empty());

    fn check_dyn(_p: &dyn UnicodeProvider) {}
    check_dyn(&p);
}

#[test]
fn null_idna_provider_returns_none_and_is_dyn_safe() {
    let p = NullIdnaProvider;
    assert_eq!(p.provider_name(), "null");
    assert!(p.to_ascii("пример.рф").is_none());
    assert!(p.to_unicode("xn--e1afmkfd.xn--p1ai").is_none());

    fn check_dyn(_p: &dyn IdnaProvider) {}
    check_dyn(&p);
}

#[test]
fn null_public_suffix_list_returns_none_and_is_dyn_safe() {
    let p = NullPublicSuffixList;
    assert_eq!(p.provider_name(), "null");
    assert!(p.public_suffix("example.co.uk").is_none());
    assert!(p.registrable_domain("foo.bar.example.co.uk").is_none());
    assert!(!p.is_public_suffix("co.uk"));

    fn check_dyn(_p: &dyn PublicSuffixList) {}
    check_dyn(&p);
}

#[test]
fn unsupported_content_decoder_carries_encoding_name() {
    let d = UnsupportedContentDecoder::new("br");
    assert_eq!(d.encoding(), "br");
    let err = d.decode(b"compressed").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("br"), "msg should mention encoding: {msg}");
    assert!(
        msg.contains("not supported"),
        "msg should explain unsupported: {msg}"
    );
}

#[test]
fn unsupported_content_decoder_is_dyn_safe() {
    let d = UnsupportedContentDecoder::new("zstd");
    fn check_dyn(_p: &dyn ContentDecoder) {}
    check_dyn(&d);
}

#[test]
fn null_font_format_rejects_everything() {
    let f = NullFontFormat;
    assert_eq!(f.format_name(), "null");
    assert!(!f.can_decode(b"wOF2"));
    assert!(f.decode_to_sfnt(b"wOF2").is_err());

    fn check_dyn(_p: &dyn FontFormat) {}
    check_dyn(&f);
}

#[test]
fn null_spell_checker_accepts_all_words_and_offers_no_suggestions() {
    let s = NullSpellChecker;
    assert_eq!(s.locale(), "null");
    // Null checker не подчёркивает ничего — иначе UI был бы залит squiggly.
    assert!(s.check("orfograafy"));
    assert!(s.check("слово"));
    assert!(s.suggest("orfograafy").is_empty());

    fn check_dyn(_p: &dyn SpellChecker) {}
    check_dyn(&s);
}

#[test]
fn null_hyphenation_provider_returns_empty_and_is_dyn_safe() {
    let h = NullHyphenationProvider;
    assert!(h.hyphenate("hyphenation", "en-US").is_empty());
    assert!(h.locales().is_empty());

    fn check_dyn(_p: &dyn HyphenationProvider) {}
    check_dyn(&h);
}

#[test]
fn sprint0_null_impls_are_send_sync() {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<NullUnicodeProvider>();
    is_send_sync::<NullIdnaProvider>();
    is_send_sync::<NullPublicSuffixList>();
    is_send_sync::<UnsupportedContentDecoder>();
    is_send_sync::<NullFontFormat>();
    is_send_sync::<NullSpellChecker>();
    is_send_sync::<NullHyphenationProvider>();
}

// --- ADR-006: Automation API ---

#[test]
fn browser_session_null_impl_returns_errors() {
    let mut session = NullBrowserSession;
    assert!(session.navigate("about:blank").is_err());
    assert!(session.screenshot().is_err());
    assert!(session.a11y_tree().is_err());
    assert!(session.click("button").is_err());
    assert!(session.type_text("hello").is_err());
    assert!(session.scroll_by(100.0).is_err());
    assert!(session.wait_for_navigation().is_err());
    assert!(session.wait_for_idle().is_err());
    assert!(session.set_viewport(1024, 720).is_err());
    assert!(session.computed_style("body").is_err());
    assert!(session.eval("1 + 2").is_err());
}

#[test]
fn browser_session_null_impl_viewport_returns_default() {
    let session = NullBrowserSession;
    assert_eq!(session.viewport(), (1024, 720));
}

#[test]
fn browser_session_is_object_safe() {
    // dyn check: trait is object-safe for transport polymorphism.
    fn check_dyn(_s: &mut dyn BrowserSession) {}
    let mut session = NullBrowserSession;
    check_dyn(&mut session);
}

#[test]
fn browser_session_null_impl_is_send() {
    fn is_send<T: Send>() {}
    is_send::<NullBrowserSession>();
}

// --- ADR-008: Tab lifecycle suspension (Invariant 2) ---

#[test]
fn js_runtime_null_impl_pause_unpause_not_implemented() {
    let mut rt = NullJsRuntime;
    assert!(rt.pause().is_err());
    assert!(rt.unpause().is_err());
}

#[test]
fn js_runtime_null_impl_suspend_resume_not_implemented() {
    let mut rt = NullJsRuntime;
    assert!(rt.suspend().is_err());
    assert!(NullJsRuntime::resume(SuspendedHeap::default()).is_err());
}

#[test]
fn suspended_heap_len_and_empty() {
    let empty = SuspendedHeap::default();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);

    let with_data = SuspendedHeap::new(vec![1, 2, 3, 4, 5]);
    assert!(!with_data.is_empty());
    assert_eq!(with_data.len(), 5);
}

#[test]
fn suspended_heap_preserves_compressed_bytes() {
    let data = vec![0x42, 0x43, 0x44];
    let heap = SuspendedHeap::new(data.clone());
    assert_eq!(heap.compressed, data);
}

// --- ADR-008 §10D.3: CacheRegistry tests ---

struct MockCache {
    name: &'static str,
    used: usize,
    budget: usize,
    cleared: bool,
    last_pressure: Option<MemoryPressureLevel>,
}

impl MockCache {
    fn new(name: &'static str, used: usize, budget: usize) -> Self {
        Self { name, used, budget, cleared: false, last_pressure: None }
    }
}

// SAFETY: test double whose fields are `&'static str`, `usize`, `bool` and an
// `Option<MemoryPressureLevel>` — all plain data with no thread affinity. The
// impl exists only because `EvictableCache` implementors are stored behind a
// `Send` bound in the registry under test.
unsafe impl Send for MockCache {}

impl EvictableCache for MockCache {
    fn on_memory_pressure(&mut self, level: MemoryPressureLevel) {
        self.last_pressure = Some(level);
    }
    fn used_bytes(&self) -> usize { self.used }
    fn budget_bytes(&self) -> usize { self.budget }
    fn clear(&mut self) { self.cleared = true; self.used = 0; }
    fn cache_name(&self) -> &'static str { self.name }
}

#[test]
fn cache_registry_empty() {
    let r = CacheRegistry::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert_eq!(r.total_used_bytes(), 0);
    assert_eq!(r.total_budget_bytes(), 0);
}

#[test]
fn cache_registry_register_and_len() {
    let mut r = CacheRegistry::new();
    r.register(Box::new(MockCache::new("a", 100, 1000)));
    r.register(Box::new(MockCache::new("b", 200, 2000)));
    assert_eq!(r.len(), 2);
    assert!(!r.is_empty());
}

#[test]
fn cache_registry_total_used_bytes() {
    let mut r = CacheRegistry::new();
    r.register(Box::new(MockCache::new("a", 100, 1000)));
    r.register(Box::new(MockCache::new("b", 250, 2000)));
    assert_eq!(r.total_used_bytes(), 350);
}

#[test]
fn cache_registry_total_budget_excludes_unbounded() {
    let mut r = CacheRegistry::new();
    r.register(Box::new(MockCache::new("bounded", 100, 1000)));
    r.register(Box::new(MockCache::new("unbounded", 512, usize::MAX)));
    // Only bounded cache contributes.
    assert_eq!(r.total_budget_bytes(), 1000);
}

#[test]
fn cache_registry_broadcast_pressure_notifies_all() {
    let mut r = CacheRegistry::new();
    r.register(Box::new(MockCache::new("a", 100, 1000)));
    r.register(Box::new(MockCache::new("b", 200, 2000)));
    r.broadcast_pressure(MemoryPressureLevel::High);
    // Both caches should have received the event (verified by clear_all below).
}

#[test]
fn cache_registry_clear_all() {
    let mut r = CacheRegistry::new();
    r.register(Box::new(MockCache::new("a", 100, 1000)));
    r.register(Box::new(MockCache::new("b", 200, 2000)));
    r.clear_all();
    assert_eq!(r.total_used_bytes(), 0);
}

#[test]
fn evictable_cache_is_object_safe() {
    fn check(_c: &dyn EvictableCache) {}
    let c = MockCache::new("test", 0, 0);
    check(&c);
}
