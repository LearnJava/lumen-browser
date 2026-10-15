use super::*;
use lumen_core::url::Url;

/// Tiny stand-in for the real PSL (the crate must not pull `lumen-storage` in).
struct TestPsl;

const SUFFIXES: &[&str] = &["com", "ru", "org", "co.uk", "gov.ru"];

impl PublicSuffixList for TestPsl {
    fn public_suffix<'a>(&self, domain: &'a str) -> Option<&'a str> {
        SUFFIXES
            .iter()
            .filter(|s| domain == **s || domain.ends_with(&format!(".{s}")))
            .max_by_key(|s| s.len())
            .map(|s| &domain[domain.len() - s.len()..])
    }
    fn registrable_domain<'a>(&self, domain: &'a str) -> Option<&'a str> {
        let suffix = self.public_suffix(domain)?;
        let head = domain.strip_suffix(suffix)?.strip_suffix('.')?;
        let label_start = head.rfind('.').map_or(0, |i| i + 1);
        Some(&domain[label_start..])
    }
    fn is_public_suffix(&self, domain: &str) -> bool {
        SUFFIXES.contains(&domain)
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
}

fn key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[9u8; 32])
}

fn trusted() -> Vec<(&'static str, [u8; 32])> {
    vec![("pack-test", key().verifying_key().to_bytes())]
}

fn entry(id: &str, host: &str, action: Action) -> CompatEntry {
    CompatEntry {
        id: id.into(),
        root_cause: "site-bug".into(),
        matches: vec![host.into()],
        site_class: SiteClass::General,
        action,
        bug: "BUG-1".into(),
        review_by: "2027-01-31".into(),
        tests: vec!["tests/compat/a.html".into()],
        test_exemption: None,
        engine_max: None,
    }
}

fn css(s: &str) -> Action {
    Action::Css { css: s.into() }
}

fn pack(entries: Vec<CompatEntry>) -> CompatPack {
    CompatPack { format: PACK_FORMAT, revision: 1, entries, key_id: String::new(), signature: String::new() }
}

fn rejected(entries: Vec<CompatEntry>) -> String {
    match pack(entries).validate(&TestPsl) {
        Err(PackError::Invalid { reason, .. }) => reason,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn fixture() -> CompatPack {
    pack(vec![
        entry("a-css", "shop.test-site.com", css("body{}")),
        entry("b-issue", "test-site.com", Action::KnownIssue { message: "slow".into() }),
        entry("c-other", "other.com", css("p{}")),
    ])
}

fn signed_bytes(p: &mut CompatPack) -> Vec<u8> {
    p.sign("pack-test", &key());
    serde_json::to_vec(p).unwrap()
}

#[test]
fn test_host_matches_exactly_two_entries_in_action_order() {
    let mut p = fixture();
    // Put the known_issue first in the file: the result must still be css-first.
    p.entries.swap(0, 1);
    let ids: Vec<_> = p.entries_for_host("shop.test-site.com").iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["a-css", "b-issue"]);
    assert_eq!(p.entries_for_host("www.test-site.com").len(), 1);
}

#[test]
fn match_respects_label_boundary_and_trailing_dot() {
    let p = fixture();
    assert!(p.entries_for_host("evil-test-site.com").is_empty());
    assert!(p.entries_for_host("test-site.com.evil.org").is_empty());
    assert_eq!(p.entries_for_host("test-site.com.").len(), 1);
}

#[test]
fn match_by_url_only_for_http_and_https() {
    let p = fixture();
    assert_eq!(p.entries_for_url(&Url::parse("https://shop.test-site.com/x").unwrap()).len(), 2);
    assert!(p.entries_for_url(&Url::parse("ftp://shop.test-site.com/x").unwrap()).is_empty());
}

#[test]
fn signed_pack_round_trips() {
    let mut p = fixture();
    let bytes = signed_bytes(&mut p);
    let got = CompatPack::parse_signed(&bytes, &trusted(), &TestPsl).unwrap();
    assert_eq!(got, p);
}

#[test]
fn one_changed_byte_breaks_the_signature() {
    let mut p = fixture();
    let bytes = signed_bytes(&mut p);
    let text = String::from_utf8(bytes).unwrap();
    let tampered = text.replacen("body{}", "body{!", 1);
    assert_ne!(tampered, text);
    assert_eq!(
        CompatPack::parse_signed(tampered.as_bytes(), &trusted(), &TestPsl),
        Err(PackError::Signature(ManifestVerifyError::SignatureMismatch))
    );
}

#[test]
fn signature_under_another_domain_tag_is_rejected() {
    let mut p = fixture();
    p.key_id = "pack-test".into();
    p.signature = sign_tagged("lumen-update-manifest-v1", &p.signing_body(), &key());
    assert_eq!(p.verify_with_keys(&trusted()), Err(PackError::Signature(ManifestVerifyError::SignatureMismatch)));
}

#[test]
fn unknown_key_and_empty_production_keys_are_rejected() {
    let mut p = fixture();
    let bytes = signed_bytes(&mut p);
    assert_eq!(
        CompatPack::parse_signed(&bytes, &[], &TestPsl),
        Err(PackError::Signature(ManifestVerifyError::UnknownKeyId))
    );
    assert!(CompatPack::parse_signed_production(&bytes, &TestPsl).is_err());
}

#[test]
fn unknown_action_and_unknown_field_do_not_parse() {
    let mk = |action: &str, extra: &str| {
        format!(
            r#"{{"format":1,"revision":1,"entries":[{{"id":"x","root_cause":"site-bug","matches":["a.com"],
            "action":{action},"bug":"BUG-1","review_by":"2027-01-01","test_exemption":"manual"{extra}}}]}}"#
        )
    };
    let ok = mk(r#"{"kind":"css","css":"p{}"}"#, "");
    assert!(CompatPack::parse_unsigned(ok.as_bytes(), &TestPsl).is_ok());
    let unknown_action = mk(r#"{"kind":"run_script","src":"alert(1)"}"#, "");
    assert!(matches!(CompatPack::parse_unsigned(unknown_action.as_bytes(), &TestPsl), Err(PackError::Parse(_))));
    let unknown_field = mk(r#"{"kind":"css","css":"p{}","extra":1}"#, "");
    assert!(matches!(CompatPack::parse_unsigned(unknown_field.as_bytes(), &TestPsl), Err(PackError::Parse(_))));
    let unknown_entry_field = mk(r#"{"kind":"css","css":"p{}"}"#, r#","remote_url":"x""#);
    assert!(matches!(CompatPack::parse_unsigned(unknown_entry_field.as_bytes(), &TestPsl), Err(PackError::Parse(_))));
}

#[test]
fn patterns_broader_than_etld_plus_one_are_rejected() {
    for bad in [
        "*.google.*", "*.example.com", "google.*", "com", "co.uk", "gov.ru", ".example.com", "example.com.",
        "Example.com", "пример.рф", "10.0.0.1", "a..com", "",
    ] {
        let reason = rejected(vec![entry("x", bad, css("p{}"))]);
        assert!(reason.starts_with("match"), "{bad:?}: {reason}");
    }
    let ok = pack(vec![entry("x", "google.com", css("p{}")), entry("y", "mail.example.co.uk", css("p{}"))]);
    assert!(ok.validate(&TestPsl).is_ok());
}

#[test]
fn bank_payment_gov_accept_only_identity_css_known_issue() {
    let protection = Action::Protection { name: "allow-trackers".into() };
    // Declared class.
    let mut e = entry("x", "somebank.com", protection.clone());
    e.site_class = SiteClass::Bank;
    assert!(rejected(vec![e]).contains("bank/payment/gov"));
    // Mislabelled as general, but the host is on the built-in list.
    let transform = Action::JsTransform { id: "fix".into() };
    assert!(rejected(vec![entry("x", "online.sberbank.ru", transform)]).contains("bank/payment/gov"));
    assert!(rejected(vec![entry("x", "nalog.gov.ru", protection)]).contains("bank/payment/gov"));
    let mut ok = entry("x", "somebank.com", Action::Identity { profile: IdentityProfile::ChromeUnbranded });
    ok.site_class = SiteClass::Bank;
    let issue = Action::KnownIssue { message: "m".into() };
    let allowed = pack(vec![ok, entry("y", "vtb.ru", css("p{}")), entry("z", "gosuslugi.ru", issue)]);
    assert!(allowed.validate(&TestPsl).is_ok());
}

#[test]
fn bookkeeping_fields_are_enforced() {
    let mut e = entry("x", "a.com", css("p{}"));
    e.bug = "bug-1".into();
    assert!(rejected(vec![e]).contains("bug"));
    for date in ["2027-13-01", "2027-02-30", "2027-1-01", "2027-+1-01", "soon", "1999-01-01"] {
        let mut e = entry("x", "a.com", css("p{}"));
        e.review_by = date.into();
        assert!(rejected(vec![e]).contains("review_by"), "{date}");
    }
    let mut leap = entry("x", "a.com", css("p{}"));
    leap.review_by = "2028-02-29".into();
    assert!(pack(vec![leap]).validate(&TestPsl).is_ok());

    let mut no_tests = entry("x", "a.com", css("p{}"));
    no_tests.tests.clear();
    assert!(rejected(vec![no_tests.clone()]).contains("tests"));
    no_tests.test_exemption = Some("  ".into());
    assert!(rejected(vec![no_tests.clone()]).contains("tests"));
    no_tests.test_exemption = Some("needs a real bank account".into());
    assert!(pack(vec![no_tests]).validate(&TestPsl).is_ok());

    assert!(rejected(vec![entry("x", "a.com", css("p{}")), entry("x", "b.com", css("p{}"))]).contains("duplicate"));
    assert!(rejected(vec![entry("x", "a.com", css("  "))]).contains("empty"));
    assert!(rejected(vec![entry("X!", "a.com", css("p{}"))]).contains("id"));
}

#[test]
fn engine_causes_need_engine_max() {
    let mut e = entry("x", "a.com", Action::EngineFlag { flag: "webkit-fullscreen".into() });
    e.root_cause = "engine-api".into();
    assert!(rejected(vec![e.clone()]).contains("engine_max"));
    e.engine_max = Some("0.9.0".into());
    assert!(pack(vec![e.clone()]).validate(&TestPsl).is_ok());
    e.engine_max = Some("soon".into());
    assert!(rejected(vec![e]).contains("engine_max"));
    let mut other = entry("y", "a.com", css("p{}"));
    other.engine_max = Some("0.9.0".into());
    assert!(rejected(vec![other]).contains("engine_max"));
}

#[test]
fn at_most_twenty_js_transforms() {
    let many = |n: usize| {
        (0..n)
            .map(|i| entry(&format!("t{i}"), "a.com", Action::JsTransform { id: format!("tr{i}") }))
            .collect::<Vec<_>>()
    };
    assert!(pack(many(MAX_JS_TRANSFORMS)).validate(&TestPsl).is_ok());
    assert!(matches!(pack(many(MAX_JS_TRANSFORMS + 1)).validate(&TestPsl), Err(PackError::Invalid { .. })));
}

#[test]
fn wrong_format_and_oversize_are_rejected() {
    let mut p = fixture();
    p.format = 2;
    assert_eq!(p.validate(&TestPsl), Err(PackError::UnsupportedFormat(2)));
    assert_eq!(CompatPack::parse_unsigned(&vec![b' '; MAX_PACK_BYTES + 1], &TestPsl), Err(PackError::TooLarge));
}

#[test]
fn overdue_lists_only_past_review_dates() {
    let mut p = fixture();
    p.entries[0].review_by = "2026-01-01".into();
    let ids: Vec<_> = p.overdue("2026-10-15").iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["a-css"]);
}

#[test]
fn embedded_base_copy_is_valid() {
    let raw = include_str!("../../assets/compat_pack.json");
    let base = CompatPack::parse_unsigned(raw.as_bytes(), &TestPsl).expect("base copy must validate");
    assert_eq!(CompatPack::base(&TestPsl), base);
}
