//! Единый манифест отпечатка Chrome 157 (FINGERPRINT-MANIFEST, WS-B).
//!
//! Один модуль данных, который читают все Chrome-bound слои сетевого стека:
//! H2 SETTINGS ([`crate::http::h2_settings`]), TLS ClientHello
//! ([`crate::tls::boring`]), `Accept-Encoding`/декодеры
//! ([`crate::default_content_decoders`]) и порядок заголовков
//! ([`crate::http::headers`], проверяется consistency-тестом). Не-Chrome
//! профили (Firefox/Safari/Edge/Tor/Lumen) манифест не читают.
//!
//! UA-строка и Client Hints здесь **не** дублируются — их канонический
//! источник `lumen_core::identity` (слои: core не может зависеть от
//! network); [`CHROME_157`] только ссылается на `CHROME_MAJOR`, а тест
//! [`tests::chrome_manifest_matches_identity`] сторожит согласованность.
//!
//! Значения — те, что измерены на проводе (tls.peet.ws/api/all, BUG-1826);
//! манифест фиксирует их как единственную точку правды, сам рефакторинг
//! не меняет wire-формат (контроль — byte-identical diff header-set'ов).

/// HTTP/2 SETTINGS-фрейм Chrome 157 (akamai `1:65536;2:0;4:6291456;6:262144`).
#[derive(Debug, Clone, Copy)]
pub struct H2Spec {
    /// SETTINGS_HEADER_TABLE_SIZE.
    pub header_table_size: u32,
    /// SETTINGS_ENABLE_PUSH (Chrome 157 шлёт 0).
    pub enable_push: bool,
    /// SETTINGS_MAX_CONCURRENT_STREAMS; `None` — не отправлять.
    pub max_concurrent_streams: Option<u32>,
    /// SETTINGS_INITIAL_WINDOW_SIZE.
    pub initial_window_size: u32,
    /// SETTINGS_MAX_FRAME_SIZE; `None` — не отправлять (Chrome 157 его не шлёт).
    pub max_frame_size: Option<u32>,
    /// SETTINGS_MAX_HEADER_LIST_SIZE.
    pub max_header_list_size: Option<u32>,
    /// Приращение connection receive window (stream-0 WINDOW_UPDATE после preface).
    pub connection_window_increment: u32,
}

/// TLS ClientHello-параметры Chrome 157 (BoringSSL, OpenSSL-имена списков).
#[derive(Debug, Clone, Copy)]
pub struct TlsSpec {
    /// `set_cipher_list` — TLS 1.3 suites неявно в BoringSSL, список
    /// упорядочивает ≤1.2 половину.
    pub cipher_list: &'static str,
    /// `set_sigalgs_list` — signature_algorithms.
    pub sigalgs: &'static str,
    /// `set_curves_list` — supported_groups (X25519MLKEM768 первым, Chrome ≥124).
    pub groups: &'static str,
    /// ALPN, когда h2 разрешён: `[h2, http/1.1]` в wire-формате.
    pub alpn: &'static [u8],
    /// ALPN для соединений, пришпиленных к HTTP/1.1.
    pub alpn_h1: &'static [u8],
}

/// Манифест одного мажора Chrome: версия + все wire-параметры, которые
/// антибот-слои сравнивают между собой.
#[derive(Debug, Clone, Copy)]
pub struct ChromeManifest {
    /// Мажор Chrome — обязан равняться [`lumen_core::CHROME_MAJOR`].
    pub major: &'static str,
    /// Канонический порядок имён заголовков document-навигации Chrome 157,
    /// включая Sec-Fetch-* (без `Host`/`Connection` — они транспортные,
    /// но в H1-порядке присутствуют, поэтому в списке).
    pub header_order: &'static [&'static str],
    pub h2: H2Spec,
    pub tls: TlsSpec,
    /// `Accept-Encoding` в порядке предпочтения (== порядку регистрации
    /// декодеров в [`crate::default_content_decoders`]).
    pub accept_encoding: &'static [&'static str],
}

/// Манифест текущего стабильного Chrome — single source of truth для
/// всех Chrome-bound слоёв (WS-B).
pub const CHROME_157: ChromeManifest = ChromeManifest {
    major: lumen_core::CHROME_MAJOR,
    header_order: &[
        "Host",
        "Connection",
        "Sec-CH-UA",
        "Sec-CH-UA-Mobile",
        "Sec-CH-UA-Platform",
        "Upgrade-Insecure-Requests",
        "User-Agent",
        "Accept",
        "Sec-Fetch-Site",
        "Sec-Fetch-Mode",
        "Sec-Fetch-User",
        "Sec-Fetch-Dest",
        "Accept-Encoding",
        "Accept-Language",
        "Priority",
    ],
    h2: H2Spec {
        header_table_size: 65536,
        enable_push: false,
        max_concurrent_streams: None,
        initial_window_size: 6291456,
        max_frame_size: None,
        max_header_list_size: Some(262144),
        connection_window_increment: 15_663_105,
    },
    tls: TlsSpec {
        cipher_list: "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:\
TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:\
ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:\
ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:\
ECDHE-ECDSA-AES128-SHA:ECDHE-RSA-AES128-SHA:AES128-GCM-SHA256:AES256-GCM-SHA384:\
AES128-SHA:AES256-SHA",
        sigalgs: "ecdsa_secp256r1_sha256:rsa_pss_rsae_sha256:rsa_pkcs1_sha256:\
ecdsa_secp384r1_sha384:rsa_pss_rsae_sha384:rsa_pkcs1_sha384:\
rsa_pss_rsae_sha512:rsa_pkcs1_sha512",
        groups: "X25519MLKEM768:X25519:P-256:P-384",
        alpn: b"\x02h2\x08http/1.1",
        alpn_h1: b"\x08http/1.1",
    },
    accept_encoding: &["gzip", "deflate", "br", "zstd"],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Манифест не дублирует UA/CH, но обязан с ним согласовываться:
    /// major == CHROME_MAJOR, UA и Sec-CH-UA бренды содержат тот же мажор.
    #[test]
    fn chrome_manifest_matches_identity() {
        assert_eq!(CHROME_157.major, lumen_core::CHROME_MAJOR);
        for ua in [
            lumen_core::CHROME_USER_AGENT,
            lumen_core::CHROME_LUMEN_USER_AGENT,
        ] {
            assert!(
                ua.contains(&format!("Chrome/{}", CHROME_157.major)),
                "UA {ua} не содержит Chrome/{}",
                CHROME_157.major
            );
        }
        let id = lumen_core::IdentityProfile::ChromeUnbranded.identity();
        let hints = id.low_entropy_client_hints();
        let ch_ua = hints.iter().find(|(n, _)| *n == "Sec-CH-UA").map(|(_, v)| v.clone());
        assert!(
            ch_ua.as_deref().is_some_and(|v| v.contains(&format!("\"{}\"", CHROME_157.major))),
            "Sec-CH-UA {ch_ua:?} не содержит мажор {}",
            CHROME_157.major
        );
    }

    /// Chrome/Strict arm `H2Settings::for_profile` читает манифест —
    /// значения на проводе не изменились (рефакторинг WS-B).
    #[test]
    fn h2_settings_match_manifest() {
        for profile in [
            crate::http::HttpProfile::Chrome,
            crate::http::HttpProfile::Strict,
        ] {
            let s = crate::http::H2Settings::for_profile(profile);
            assert_eq!(s.header_table_size, CHROME_157.h2.header_table_size, "{profile:?}");
            assert_eq!(s.enable_push, CHROME_157.h2.enable_push, "{profile:?}");
            assert_eq!(s.max_concurrent_streams, CHROME_157.h2.max_concurrent_streams, "{profile:?}");
            assert_eq!(s.initial_window_size, CHROME_157.h2.initial_window_size, "{profile:?}");
            assert_eq!(s.max_frame_size, CHROME_157.h2.max_frame_size, "{profile:?}");
            assert_eq!(s.max_header_list_size, CHROME_157.h2.max_header_list_size, "{profile:?}");
            assert_eq!(
                s.connection_window_increment,
                CHROME_157.h2.connection_window_increment,
                "{profile:?}"
            );
        }
    }

    /// Порядок заголовков builder'а (`headers.rs`, Chrome arm) совпадает
    /// с `CHROME_157.header_order` — манифест не дублирует таблицу,
    /// а сторожит её (выбор «consistency test вместо экстракции»
    /// зафиксирован в WS-B: экстракция OrderedTable дёрнула бы весь
    /// builder ради нуля выигрыша на проводе).
    #[test]
    fn chrome_header_order_matches_builder() {
        let id = lumen_core::IdentityProfile::ChromeLumen.identity();
        let block = crate::http::build_request_headers_with_identity(
            "example.com",
            "gzip, deflate, br, zstd",
            "",
            crate::http::HttpProfile::Chrome,
            &id,
        );
        let names: Vec<String> = block
            .split("\r\n")
            .filter_map(|line| line.split_once(':'))
            .map(|(name, _)| name.trim().to_string())
            .collect();
        let expected: Vec<String> = CHROME_157
            .header_order
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(names, expected, "порядок заголовков Chrome разошёлся с манифестом");
    }

    /// Порядок Accept-Encoding из манифеста == порядку регистрации
    /// дефолтных декодеров (join через `accept_encoding_header` повторяет
    /// манифест побайтово).
    #[test]
    fn chrome_manifest_encodings_match_clients() {
        let encodings: Vec<String> = crate::default_content_decoders()
            .iter()
            .map(|d| d.encoding().to_string())
            .collect();
        assert_eq!(
            encodings,
            CHROME_157.accept_encoding
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            "порядок декодеров разошёлся с CHROME_157.accept_encoding"
        );
        let client = crate::HttpClient::new()
            .with_content_decoders(crate::default_content_decoders());
        assert_eq!(
            client.accept_encoding_header().as_deref(),
            Some("gzip, deflate, br, zstd")
        );
    }
}
