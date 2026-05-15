//! Mixed Content classification по W3C Mixed Content
//! <https://w3c.github.io/webappsec-mixed-content/>.
//!
//! Идея: secure (HTTPS) top-level context не должен ослабляться загрузкой
//! non-secure (HTTP) sub-resource-ов — это нивелирует TLS-гарантии.
//!
//! Spec выделяет две градации:
//! - **Blockable** — scripts, css, iframes, fonts, XHR/fetch, worker, etc.
//!   Загружать **запрещено** в secure-контексте.
//! - **Optionally blockable** — images, audio, video, prefetch. Spec
//!   допускает загрузку, но рекомендует апгрейд / блокировку UA-выбором.
//!   В Lumen политика по умолчанию: optionally-blockable также блокируем,
//!   но возвращаем отдельный уровень — это даёт shell-у точку решения
//!   (например, показать «mixed content» индикатор и не падать на всех
//!   старых сайтах с `<img src=http://...>` на HTTPS-странице).
//!
//! Решение по запросу: [`classify_subresource_request`] на вход берёт
//! origin top-level документа и URL подресурса + его destination
//! (по терминологии Fetch spec — что грузит request: `Script`, `Style`,
//! `Image`, ...).
//!
//! Этот модуль — **классификатор**, не enforcer. Enforcement (отказать
//! в fetch) — отдельный слой, который примет решение по политике
//! пользователя (strict-block / allow optionally / etc.).

use crate::origin::Origin;
use lumen_core::url::Url;

/// Назначение подресурса по Fetch spec §3.2.7 «request destination» —
/// определяет, какой блокируемости подвергается mixed-content для этого
/// типа запроса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestDestination {
    /// `<script>`, worker scripts, importScripts.
    Script,
    /// `<link rel=stylesheet>`, `@import`.
    Style,
    /// `<iframe>`, `<frame>`, `<object>`, `<embed>` document/subdocument.
    Document,
    /// `@font-face` URL.
    Font,
    /// `XMLHttpRequest`, `fetch()`, EventSource, beacon, WebSocket open.
    Connect,
    /// `<img>`, `<picture>`, favicon, SVG image.
    Image,
    /// `<audio>`, `<video>`, `<track>`.
    Media,
    /// `<link rel=prefetch / preload / prerender>`.
    Prefetch,
    /// Worker module / SharedWorker / ServiceWorker bootstrap.
    Worker,
    /// Manifest, report-to, audioworklet — всё, что spec явно перечисляет как
    /// blockable, но в Phase 0 мы их не отдельно классифицируем.
    Other,
}

/// Mixed-content уровень для запроса в secure-контексте.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixedContentLevel {
    /// Sub-resource — secure (HTTPS / data: / blob: / same-origin file:) или
    /// top-level — non-secure (нет проблемы mixed content). Запрос
    /// разрешён без ограничений.
    NotMixed,
    /// Optionally blockable mixed content — image / audio / video / prefetch.
    /// Spec разрешает, но Lumen рекомендует блокировку по умолчанию.
    OptionallyBlockable,
    /// Blockable mixed content — script / style / fetch / iframe / font / worker.
    /// **Запрос обязан быть отвергнут** в secure-контексте.
    Blockable,
}

impl MixedContentLevel {
    /// Должны ли мы блокировать запрос по строгому режиму. По умолчанию
    /// в Lumen — да для обеих категорий mixed content (см. модульный комментарий).
    pub fn is_strict_blocked(self) -> bool {
        matches!(self, Self::Blockable | Self::OptionallyBlockable)
    }

    /// Должны ли мы блокировать запрос по spec-default режиму
    /// (как делают современные браузеры в дефолтном режиме):
    /// блокируем blockable, пропускаем optionally-blockable.
    pub fn is_spec_default_blocked(self) -> bool {
        matches!(self, Self::Blockable)
    }
}

/// Является ли URL "a priori authenticated" (Mixed Content spec §3.1):
/// 1) potentially trustworthy origin, ИЛИ
/// 2) UA-внутренние схемы без origin (`data:`, `blob:`, `about:`, `file:`).
///    `data:` URL-ы наследуют trust от контекста, не создают mixed-content.
fn is_authenticated_url(url: &Url) -> bool {
    let scheme = url.scheme();
    if matches!(scheme, "data" | "blob" | "about" | "file" | "javascript") {
        return true;
    }
    match Origin::from_url(url) {
        Ok(o) => o.is_potentially_trustworthy(),
        Err(_) => true,
    }
}

/// Классификация подресурса для secure top-level контекста.
///
/// Если top-level **не** secure-context — возвращаем `NotMixed` всегда
/// (нет TLS-гарантии, которую можно «смешать»).
///
/// Если subresource — authenticated URL — тоже `NotMixed`.
///
/// Иначе — категория по `destination`.
pub fn classify_subresource_request(
    top_level: &Origin,
    subresource: &Url,
    destination: RequestDestination,
) -> MixedContentLevel {
    if !top_level.is_potentially_trustworthy() {
        return MixedContentLevel::NotMixed;
    }
    if is_authenticated_url(subresource) {
        return MixedContentLevel::NotMixed;
    }
    level_for_destination(destination)
}

fn level_for_destination(destination: RequestDestination) -> MixedContentLevel {
    match destination {
        RequestDestination::Image
        | RequestDestination::Media
        | RequestDestination::Prefetch => MixedContentLevel::OptionallyBlockable,
        RequestDestination::Script
        | RequestDestination::Style
        | RequestDestination::Document
        | RequestDestination::Font
        | RequestDestination::Connect
        | RequestDestination::Worker
        | RequestDestination::Other => MixedContentLevel::Blockable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn secure_top() -> Origin {
        Origin::from_url(&url("https://example.com/")).unwrap()
    }

    fn insecure_top() -> Origin {
        Origin::from_url(&url("http://example.com/")).unwrap()
    }

    #[test]
    fn http_script_on_https_page_is_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://cdn.example.org/lib.js"),
                RequestDestination::Script,
            ),
            MixedContentLevel::Blockable
        );
    }

    #[test]
    fn http_css_on_https_page_is_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://cdn.example.org/x.css"),
                RequestDestination::Style,
            ),
            MixedContentLevel::Blockable
        );
    }

    #[test]
    fn http_iframe_on_https_page_is_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://embed.example.org/widget"),
                RequestDestination::Document,
            ),
            MixedContentLevel::Blockable
        );
    }

    #[test]
    fn http_font_on_https_page_is_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://cdn.example.org/inter.woff2"),
                RequestDestination::Font,
            ),
            MixedContentLevel::Blockable
        );
    }

    #[test]
    fn http_fetch_on_https_page_is_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://api.example.org/v1"),
                RequestDestination::Connect,
            ),
            MixedContentLevel::Blockable
        );
    }

    #[test]
    fn http_image_on_https_page_is_optionally_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://cdn.example.org/pic.png"),
                RequestDestination::Image,
            ),
            MixedContentLevel::OptionallyBlockable
        );
    }

    #[test]
    fn http_video_on_https_page_is_optionally_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://cdn.example.org/clip.mp4"),
                RequestDestination::Media,
            ),
            MixedContentLevel::OptionallyBlockable
        );
    }

    #[test]
    fn http_prefetch_on_https_page_is_optionally_blockable() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://cdn.example.org/next"),
                RequestDestination::Prefetch,
            ),
            MixedContentLevel::OptionallyBlockable
        );
    }

    #[test]
    fn https_script_on_https_page_is_not_mixed() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("https://cdn.example.org/lib.js"),
                RequestDestination::Script,
            ),
            MixedContentLevel::NotMixed
        );
    }

    #[test]
    fn wss_connect_on_https_page_is_not_mixed() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("wss://api.example.org/"),
                RequestDestination::Connect,
            ),
            MixedContentLevel::NotMixed
        );
    }

    #[test]
    fn ws_connect_on_https_page_is_blockable() {
        // Plain ws:// — non-authenticated, secure-context — blockable.
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("ws://api.example.org/"),
                RequestDestination::Connect,
            ),
            MixedContentLevel::Blockable
        );
    }

    #[test]
    fn http_script_to_loopback_on_https_page_is_not_mixed() {
        // localhost — potentially trustworthy, mixed-content не возникает.
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://localhost:3000/lib.js"),
                RequestDestination::Script,
            ),
            MixedContentLevel::NotMixed
        );
    }

    #[test]
    fn http_script_to_127_loopback_on_https_page_is_not_mixed() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("http://127.0.0.1:8080/lib.js"),
                RequestDestination::Script,
            ),
            MixedContentLevel::NotMixed
        );
    }

    #[test]
    fn data_url_on_https_page_is_not_mixed() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("data:image/png;base64,iVBORw0KGgo="),
                RequestDestination::Image,
            ),
            MixedContentLevel::NotMixed
        );
    }

    #[test]
    fn blob_url_on_https_page_is_not_mixed() {
        assert_eq!(
            classify_subresource_request(
                &secure_top(),
                &url("blob:https://example.com/uuid"),
                RequestDestination::Connect,
            ),
            MixedContentLevel::NotMixed
        );
    }

    #[test]
    fn http_anything_on_http_page_is_not_mixed() {
        // top-level не secure — концепции mixed content нет.
        assert_eq!(
            classify_subresource_request(
                &insecure_top(),
                &url("http://cdn.example.org/lib.js"),
                RequestDestination::Script,
            ),
            MixedContentLevel::NotMixed
        );
    }

    #[test]
    fn strict_blocks_optionally_blockable() {
        assert!(MixedContentLevel::OptionallyBlockable.is_strict_blocked());
        assert!(MixedContentLevel::Blockable.is_strict_blocked());
        assert!(!MixedContentLevel::NotMixed.is_strict_blocked());
    }

    #[test]
    fn spec_default_lets_optionally_blockable_through() {
        assert!(!MixedContentLevel::OptionallyBlockable.is_spec_default_blocked());
        assert!(MixedContentLevel::Blockable.is_spec_default_blocked());
        assert!(!MixedContentLevel::NotMixed.is_spec_default_blocked());
    }
}
