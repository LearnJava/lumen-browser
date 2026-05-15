//! Public Suffix List реализация `lumen_core::ext::PublicSuffixList` поверх
//! provisional crate-а [`psl`](https://docs.rs/psl). См. §5 «Provisional
//! accelerators» в плане — trait-anchor зарезервирован в Sprint 0, конкретная
//! реализация подключается здесь.
//!
//! Почему `psl`, а не `publicsuffix`:
//! - **`psl`** запекает PSL-таблицу в код во время сборки (codegen из
//!   `public_suffix_list.dat`), нет runtime I/O, нет dependencies. Идеально
//!   для embedded-таблицы в Phase 0.
//! - **`publicsuffix`** требует загрузить .dat-файл в runtime (с диска или
//!   по сети) — лишний этап init без выигрыша.
//!
//! Оба варианта покрываются «provisional, под `publicsuffix`-crate или свой
//! loader PSL.dat» в §5. Trait-anchor одинаков → переход crate→crate или
//! crate→собственная реализация прозрачен для потребителей.
//!
//! Graduation criterion (§5): реалистично — никогда. Формат PSL стабилен с
//! 2007-го, список обновляется снаружи через bump версии crate-а.
//!
//! API ожидает **ASCII-домены** (Punycode `xn--…` для IDN). Caller
//! получает их через `lumen_core::url::Url::host_ascii()`. Передача
//! Unicode-host-а напрямую даст `None` — `psl` работает по байтам.

use lumen_core::ext::PublicSuffixList;

/// Реализация `PublicSuffixList` поверх crate-а `psl` (compiled-in таблица).
///
/// Zero-state: всё статика, конструктор и `Default` дают одинаковый
/// экземпляр.
#[derive(Debug, Default, Clone, Copy)]
pub struct PslProvider;

impl PslProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl PublicSuffixList for PslProvider {
    fn public_suffix<'a>(&self, domain: &'a str) -> Option<&'a str> {
        let bytes = domain.as_bytes();
        let suffix = psl::suffix(bytes)?;
        let suffix_bytes = suffix.as_bytes();
        slice_tail(domain, suffix_bytes.len())
    }

    fn registrable_domain<'a>(&self, domain: &'a str) -> Option<&'a str> {
        let bytes = domain.as_bytes();
        let dom = psl::domain(bytes)?;
        let dom_bytes = dom.as_bytes();
        // `psl::domain` возвращает eTLD+1, только если sub-component поверх
        // suffix-а есть; для голых public suffix-ов (`co.uk`) он отдаёт
        // тот же `co.uk` — это для нас не «registrable», возвращаем None.
        if dom_bytes.len() == bytes.len() && self.is_public_suffix(domain) {
            return None;
        }
        slice_tail(domain, dom_bytes.len())
    }

    fn is_public_suffix(&self, domain: &str) -> bool {
        let bytes = domain.as_bytes();
        match psl::suffix(bytes) {
            Some(s) => s.as_bytes() == bytes,
            None => false,
        }
    }

    fn provider_name(&self) -> &'static str {
        "psl"
    }
}

/// Вернуть последние `n` байт `domain` как `&str`, проверив границу `.`
/// перед началом среза (boundary safety: чтобы `evil-example.com` не дал
/// `example.com` как «registrable» при ошибке `psl`).
fn slice_tail(domain: &str, n: usize) -> Option<&str> {
    let bytes = domain.as_bytes();
    if n == 0 || n > bytes.len() {
        return None;
    }
    let start = bytes.len() - n;
    if start == 0 || bytes[start - 1] == b'.' {
        // SAFETY: slice по UTF-8 boundary — `psl` работает с ASCII, suffix
        // никогда не пересекает multi-byte char. Но защищаемся через
        // get/&str (если что — None).
        domain.get(start..)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> PslProvider {
        PslProvider::new()
    }

    // ── public_suffix ──

    #[test]
    fn public_suffix_simple_com() {
        assert_eq!(p().public_suffix("example.com"), Some("com"));
    }

    #[test]
    fn public_suffix_with_subdomain() {
        // Самый длинный известный suffix для foo.example.com — это `com`.
        assert_eq!(p().public_suffix("foo.example.com"), Some("com"));
    }

    #[test]
    fn public_suffix_multipart_couk() {
        // `co.uk` — известный multipart suffix.
        assert_eq!(p().public_suffix("example.co.uk"), Some("co.uk"));
        assert_eq!(p().public_suffix("a.b.example.co.uk"), Some("co.uk"));
    }

    #[test]
    fn public_suffix_idn_punycode() {
        // `.рф` в Punycode = xn--p1ai.
        assert_eq!(
            p().public_suffix("xn--e1afmkfd.xn--p1ai"),
            Some("xn--p1ai")
        );
    }

    #[test]
    fn public_suffix_unknown_returns_icann_default() {
        // Для unknown TLD `psl` возвращает single-label fallback —
        // последний label. Это spec PSL §5 «if no rules match, the
        // prevailing rule is *». Не None.
        let r = p().public_suffix("foo.zzznoexist");
        assert_eq!(r, Some("zzznoexist"));
    }

    // ── registrable_domain ──

    #[test]
    fn registrable_domain_simple() {
        assert_eq!(p().registrable_domain("example.com"), Some("example.com"));
    }

    #[test]
    fn registrable_domain_with_subdomain() {
        assert_eq!(
            p().registrable_domain("foo.bar.example.com"),
            Some("example.com")
        );
    }

    #[test]
    fn registrable_domain_multipart_couk() {
        assert_eq!(
            p().registrable_domain("a.b.example.co.uk"),
            Some("example.co.uk")
        );
    }

    #[test]
    fn registrable_domain_for_bare_public_suffix_is_none() {
        // `co.uk` сам по себе — public suffix, registrable части над ним нет.
        assert_eq!(p().registrable_domain("co.uk"), None);
    }

    #[test]
    fn registrable_domain_idn() {
        assert_eq!(
            p().registrable_domain("xn--e1afmkfd.xn--p1ai"),
            Some("xn--e1afmkfd.xn--p1ai")
        );
        assert_eq!(
            p().registrable_domain("www.xn--e1afmkfd.xn--p1ai"),
            Some("xn--e1afmkfd.xn--p1ai")
        );
    }

    // ── is_public_suffix ──

    #[test]
    fn is_public_suffix_known_tld() {
        assert!(p().is_public_suffix("com"));
        assert!(p().is_public_suffix("uk"));
        assert!(p().is_public_suffix("co.uk"));
        assert!(p().is_public_suffix("xn--p1ai")); // .рф
    }

    #[test]
    fn is_public_suffix_false_for_registrable() {
        assert!(!p().is_public_suffix("example.com"));
        assert!(!p().is_public_suffix("example.co.uk"));
    }

    // ── provider_name ──

    #[test]
    fn provider_name_is_psl() {
        assert_eq!(p().provider_name(), "psl");
    }

    // ── slice_tail safety ──

    #[test]
    fn slice_tail_rejects_unaligned_boundary() {
        // Если suffix.len() = 7 (example), но в "anexample" перед ним нет
        // точки → slice_tail должен вернуть None. Это защита от
        // potential bug-ов в `psl`.
        assert!(slice_tail("anexample", 7).is_none());
    }

    #[test]
    fn slice_tail_accepts_aligned_dot_boundary() {
        assert_eq!(slice_tail("an.example", 7), Some("example"));
    }

    #[test]
    fn slice_tail_full_length_ok() {
        assert_eq!(slice_tail("example", 7), Some("example"));
    }

    #[test]
    fn slice_tail_zero_or_overflow_is_none() {
        assert!(slice_tail("example", 0).is_none());
        assert!(slice_tail("example", 100).is_none());
    }

    // ── dyn-safety ──

    #[test]
    fn psl_provider_is_send_sync_and_dyn() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PslProvider>();
        fn check_dyn(_p: &dyn PublicSuffixList) {}
        let p = PslProvider::new();
        check_dyn(&p);
    }
}
