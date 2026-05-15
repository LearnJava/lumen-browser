//! Точки расширения: trait-ы с возможностью разных реализаций.
//!
//! Каждый trait — это место, куда можно подложить альтернативу (другой бэкенд,
//! mock для тестов, плагин-обёртка). Реализации живут в своих крейтах
//! (например, NetworkTransport — в lumen-network).
//!
//! Trait-ы определены здесь централизованно, чтобы граф зависимостей не
//! раздувался: потребитель зависит только от lumen-core и выбранной
//! реализации, а не от всех альтернатив.

use std::net::SocketAddr;
use std::path::PathBuf;

use crate::error::Result;
use crate::event::Event;
use crate::url::Url;

/// Сетевой транспорт. Подменяется на mock для тестов или на альтернативный стек.
pub trait NetworkTransport: Send + Sync {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>>;
}

/// Приёмник событий из подсистем (network, навигация, вкладки).
///
/// Реализует принцип №4 «каждый исходящий байт виден»: транспорты эмитят
/// `Event::RequestStarted` / `RequestCompleted` / `RequestBlocked`, а
/// наблюдатель (shell, network-log UI, тесты, плагины) получает их через
/// единый интерфейс. Реализация шины (EventBus) появится позже, когда
/// потребителей станет больше одного; пока — single sink, передаваемый явно
/// в подсистему при конструировании.
///
/// `&self` без `&mut`: типичная реализация — `Mutex<Vec<Event>>` или channel,
/// и каждый `emit` атомарен. `Send + Sync` — sink можно делить между потоками
/// (фоновая загрузка favicon + main thread).
///
/// Принимаем `&Event` (а не `Event` по значению): caller обычно не нуждается
/// в Event после emit, но и платить за clone там, где sink его не сохраняет
/// (например, счётчик), не должен.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &Event);
}

/// EventSink, который молча игнорирует все события. Дефолт для подсистем,
/// у которых наблюдатель не подключён (тесты, headless-режимы). Применять
/// через `Arc::new(NoopEventSink)`, чтобы избавить hot-path от `Option`-веток.
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: &Event) {}
}

/// Хранилище ключ/значение для cookies, истории, кэша.
///
/// Все операции принимают `origin` и `top_level_site` для партиционирования
/// данных по источнику (cookie isolation, storage partitioning). `None` означает
/// глобальный профильный namespace (история, настройки).
pub trait StorageBackend: Send + Sync {
    fn get(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<Option<Vec<u8>>>;

    fn put(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
        value: &[u8],
    ) -> Result<()>;

    fn delete(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<()>;

    /// Перечислить все ключи в данном (origin, top_level_site) partition.
    fn list_keys(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
    ) -> Result<Vec<String>>;
}

/// Поисковая система для omnibox.
pub trait SearchProvider: Send + Sync {
    fn name(&self) -> &str;
    fn query_url(&self, query: &str) -> Url;
}

/// Источник списка фильтров рекламы / трекеров.
///
/// Отвечает за подгрузку текста правил (EasyList, uBlock-формат). Применение
/// этих правил к конкретному URL — задача [`RequestFilter`]; разделение
/// сделано намеренно, чтобы загрузчик и matcher жили в разных слоях
/// (потребитель `HttpClient` зависит только от `RequestFilter`).
pub trait FilterListSource: Send + Sync {
    fn name(&self) -> &str;
    fn fetch_rules(&self) -> Result<String>;
}

/// Решение «блокировать ли исходящий запрос». Реализация смотрит URL и
/// возвращает `None` для разрешённых, `Some(reason)` для блокируемых.
///
/// `reason` попадает в [`Event::RequestBlocked`](crate::event::Event)
/// и в текст возвращаемой ошибки — это пользовательская строка для UI
/// (network log: «✗ <url> (tracker)»), не машинно-читаемый код.
///
/// Отделено от [`FilterListSource`] намеренно: типичная полная цепочка —
/// `FilterListSource` (текст правил) → парсер/индекс правил → `RequestFilter`
/// (per-URL решение). `HttpClient` зависит только от `RequestFilter` и
/// ничего не знает о формате правил.
pub trait RequestFilter: Send + Sync {
    fn should_block(&self, url: &Url) -> Option<String>;
}

/// DNS-резолвер: hostname → список IP-адресов (с портом, готовых к connect).
///
/// Trait-точка вместо прямого `(host, port).to_socket_addrs()` нужна по двум
/// причинам: (1) тестируемость — mock-resolver возвращает loopback-адреса
/// без реального DNS-вызова; (2) подмена бэкенда — поверх системного
/// resolver-а в Phase 2+ появятся `CachedDnsResolver` (использует
/// `lumen-storage::DnsCache` для TTL-кеша), `DohResolver` (DNS-over-HTTPS
/// через `lumen-network::HttpClient`), `DotResolver` (DNS-over-TLS) — все
/// под одной trait-сигнатурой.
///
/// Принципы:
/// - `port` пробрасывается в SocketAddr-ы (не отдельно), чтобы вызывающий
///   слой (`HttpClient`) мог сразу `TcpStream::connect_timeout(&addr, ...)`
///   без склейки;
/// - возврат `Vec<SocketAddr>`, а не одиночный — DNS round-robin (`A` /
///   `AAAA` могут отдать несколько записей) разрешается `HttpClient` сам
///   (try-each до первого успешного connect);
/// - пустой `Vec` = NXDOMAIN / ошибка resolve — реализация может либо
///   вернуть `Err(...)`, либо пустой список; потребители трактуют оба
///   варианта одинаково («не смогли получить адрес»).
pub trait DnsResolver: Send + Sync {
    /// Разрешить hostname в список SocketAddr с указанным port.
    /// Hostname может быть Punycode-формой (`xn--…`) — резолверу всё равно.
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>>;
}

/// HSTS-политика: должны ли HTTP-запросы к данному host принудительно
/// upgrade-иться на HTTPS, и как сохранять policy из ответа.
///
/// RFC 6797 (HTTP Strict Transport Security). Trait-точка нужна, чтобы
/// `HttpClient` (`lumen-network`) не зависел напрямую от `HstsStore`
/// (`lumen-storage`) — реальный SQLite-backed store живёт в storage,
/// а network знает только про этот контракт. Тесты могут подложить
/// in-memory mock без SQLite. Аналогично DnsResolver / EncodingDetector.
///
/// **Семантика без `Result`** — fail-open: ошибка БД (поломанный диск,
/// заблокированная mutex) не должна валить fetch. `is_https_only` возвращает
/// `false` при любой проблеме (не upgrade-им сомнительный host), а
/// `record_sts` тихо проглатывает ошибки persistence (best-effort —
/// при повторе сервер всё равно пришлёт STS header). Это та же логика,
/// что у браузеров: HSTS — soft policy, не блокирующая.
///
/// Реализация в `lumen-storage::hsts::HstsStore`. RFC-семантика
/// `max-age=0` (снять HSTS) обрабатывается на стороне реализации —
/// trait просто пробрасывает значение.
pub trait HstsEnforcement: Send + Sync {
    /// Должен ли HTTP-запрос к `host` (ASCII / Punycode) быть переписан в
    /// HTTPS? Учитывает entries с `includeSubDomains` (longest-suffix-match
    /// по родительским доменам). `now_unix` — текущее время для фильтрации
    /// истёкших entries.
    fn is_https_only(&self, host: &str, now_unix: i64) -> bool;

    /// Записать HSTS policy из заголовка `Strict-Transport-Security`.
    /// `host` — ASCII / Punycode. Реализация обязана трактовать
    /// `max_age = 0` как «снять HSTS для этого host» (RFC 6797 §6.1.1).
    fn record_sts(
        &self,
        host: &str,
        max_age: u64,
        include_subdomains: bool,
        preload: bool,
        now_unix: i64,
    );
}

/// HTTP authentication scheme, разрешённый `HttpClient` для re-request
/// после 401 Unauthorized. `Digest` предпочитается над `Basic`, когда
/// сервер предлагает оба (RFC 7235 §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpAuthScheme {
    /// RFC 7617 `Basic` — base64(user:pass). По сети идёт в plain text
    /// (на HTTPS — приемлемо, на HTTP — пароль виден active attacker).
    Basic,
    /// RFC 7616 `Digest` — challenge-response с MD5 или SHA-256, nonce-based.
    /// Пароль по сети не уходит; для серверов, не поддерживающих TLS,
    /// — единственный приемлемый вариант.
    Digest,
}

impl HttpAuthScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Digest => "Digest",
        }
    }
}

/// Запрос учётных данных от credential-провайдера. Передаётся в
/// [`HttpCredentialProvider::credentials`] после получения `401 Unauthorized`.
///
/// Провайдер видит origin (`scheme://host[:port]`), realm (свободная строка
/// из header `WWW-Authenticate`, в UI обычно показывается как «область» —
/// например, `"Admin Area"`) и scheme. Детали challenge (nonce, qop,
/// algorithm) скрыты — провайдеру они не нужны, response-digest формирует
/// сам HTTP-стек.
///
/// `realm` может быть пустой строкой: RFC 7616 §3.3 допускает realm-less
/// challenge для одиночного приложения, и тогда провайдер ищет creds по
/// origin-у целиком.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpAuthChallenge {
    pub origin: String,
    pub realm: String,
    pub scheme: HttpAuthScheme,
}

/// Учётные данные для HTTP auth: username + plaintext password.
///
/// `password` хранится открыто, потому что и Basic, и Digest требуют
/// именно plain-text для построения header-а (Digest хэширует на
/// клиенте — pre-hashed значения серверу не сообщить). Реализация
/// провайдера обязана сама позаботиться о zeroing-out (если важно).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpCredentials {
    pub username: String,
    pub password: String,
}

/// Поставщик учётных данных HTTP-auth.
///
/// Вызывается `HttpClient`-ом один раз на каждый `401 Unauthorized` —
/// до retry. Возврат `None` означает «у меня нет creds для этого
/// (origin, realm, scheme)»; клиент тогда пробрасывает 401 как ошибку
/// наверх, без re-request.
///
/// Типичные реализации:
/// - `StaticCredentialProvider` — фиксированная пара `user/pass` для
///   тестов / CI / curl-style `-u user:pass`;
/// - UI-popup (P4): показывает диалог «Enter credentials for <realm>
///   on <origin>», ответ кэшируется в памяти на время сессии;
/// - keyring/secret-service (платформенный): macOS Keychain, libsecret
///   на Linux, Credential Manager на Windows.
///
/// Trait-точка вместо прямого `HttpClient::with_credentials(user, pass)`
/// нужна потому, что credentials per-origin-per-realm: один HttpClient
/// может ходить по разным сайтам, у каждого свой login.
pub trait HttpCredentialProvider: Send + Sync {
    fn credentials(&self, challenge: &HttpAuthChallenge) -> Option<HttpCredentials>;
}

/// Определение кодировки HTML-документа. Для кириллицы критично уметь
/// детектировать Windows-1251 и KOI8-R (см. §10.1).
pub trait EncodingDetector: Send + Sync {
    /// Возвращает имя кодировки (`"utf-8"`, `"windows-1251"`, …) или None,
    /// если уверенности недостаточно.
    fn detect(&self, bytes: &[u8], content_type_hint: Option<&str>) -> Option<&'static str>;
}

/// Начертание face-а: `font-style` из CSS Fonts L4. Phase 0 — три
/// дискретных значения; oblique-angle (`oblique 20deg`) и variable-axis
/// `slnt` отложены.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

impl FontStyle {
    /// Парсит CSS-ключевое слово `normal | italic | oblique` (case-insensitive).
    /// `oblique <angle>` редуцируется до `Oblique` (угол игнорируется).
    pub fn parse_keyword(s: &str) -> Option<Self> {
        let kw = s.trim();
        let kw = kw.split_ascii_whitespace().next()?;
        if kw.eq_ignore_ascii_case("normal") {
            Some(FontStyle::Normal)
        } else if kw.eq_ignore_ascii_case("italic") {
            Some(FontStyle::Italic)
        } else if kw.eq_ignore_ascii_case("oblique") {
            Some(FontStyle::Oblique)
        } else {
            None
        }
    }
}

/// Метаданные одного face-а в индексе шрифтов.
///
/// Один family может иметь несколько face-ов (Regular / Bold / Italic /
/// Bold Italic / Light / ExtraBold / …). Этот struct — то, что matcher
/// использует, чтобы выбрать face под `font-style` + `font-weight` из CSS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceRecord {
    /// Family name, нормализованный к тому же регистру, в котором лежит
    /// в индексе. Может отличаться от того, что попросил CSS — индекс
    /// case-insensitive (CSS Fonts L4 §4.3).
    pub family: String,
    /// `usWeightClass` из OS/2 (CSS-совместимый, 1..1000). По умолчанию
    /// `400` (Regular), если OS/2 нет.
    pub weight: u16,
    /// `font-style` face-а — Normal / Italic / Oblique.
    pub style: FontStyle,
    /// Путь к файлу шрифта (`.ttf` / `.otf`).
    pub path: PathBuf,
}

/// Источник системных шрифтов. Реализация — в `lumen-font::system_fonts`.
///
/// CSS-каскад даёт `font-family: ["Roboto", "Arial", sans-serif]` — приоритетный
/// список; rasterizer должен решить, какой реальный файл `.ttf` загрузить.
/// `FontProvider` отделяет «как найти шрифт на этой ОС» от «что с ним делать
/// дальше» (распарсить, растеризовать, добавить в атлас).
///
/// Имена сравниваются ASCII-case-insensitive: CSS `"Times New Roman"` должен
/// найти файл, у которого family name записан как `"Times New Roman"` или
/// `"TIMES NEW ROMAN"` — спецификация (CSS Fonts L4 §4.3) явно требует
/// case-insensitive matching.
///
/// API:
/// - [`FontProvider::lookup_family`] — все пути к файлам семейства (для
///   совместимости с кодом, которому достаточно одного face-а).
/// - [`FontProvider::lookup_faces`] — face-ы с метаданными (weight / style)
///   для font matcher-а.
/// - [`FontProvider::pick_face`] — выбор лучшего face-а по CSS Fonts L4 §5.2
///   с дефолтной реализацией поверх `lookup_faces`.
///
/// Codepoint coverage lookup (для эмодзи / CJK fallback) отложен —
/// добавим как отдельный метод, когда пойдёт реальная страница.
pub trait FontProvider: Send + Sync {
    /// Найти все пути к файлам шрифтов, объявленным под данным family name.
    /// Пустой Vec — семейство не найдено.
    fn lookup_family(&self, family: &str) -> Vec<PathBuf>;

    /// Имена всех известных семейств. Для отладки и тестов; в production
    /// потребители используют `lookup_family` или `pick_face`.
    fn list_families(&self) -> Vec<String>;

    /// Все face-ы данного семейства с метаданными. Default-реализация
    /// синтезирует «Regular 400 Normal» для каждого пути из `lookup_family`
    /// — это backward-compat для провайдеров без OS/2-парсинга;
    /// продакшн-индекс ([`super::FontProvider`]'s `SystemFontIndex`)
    /// переопределяет на реальные значения из таблицы OS/2.
    fn lookup_faces(&self, family: &str) -> Vec<FaceRecord> {
        self.lookup_family(family)
            .into_iter()
            .map(|path| FaceRecord {
                family: family.to_string(),
                weight: 400,
                style: FontStyle::Normal,
                path,
            })
            .collect()
    }

    /// Выбрать face, наиболее подходящий запрошенному `(weight, style)` —
    /// CSS Fonts L4 §5.2. Default-реализация работает поверх
    /// [`FontProvider::lookup_faces`] и [`match_face`]; переопределять
    /// нужно только если у реализации есть нативный matcher (DirectWrite /
    /// CoreText / Fontconfig).
    fn pick_face(&self, family: &str, weight: u16, style: FontStyle) -> Option<FaceRecord> {
        let faces = self.lookup_faces(family);
        match_face(&faces, weight, style).cloned()
    }
}

/// CSS Fonts L4 §5.2 алгоритм матчинга — извлечён из trait-а в свободную
/// функцию, чтобы потребитель мог звать его на собственной коллекции face-ов
/// (например, для FaceCascade в font-fallback chain).
///
/// Порядок: сначала фильтр по `style` (italic > oblique > normal приоритет
/// зависит от desired), затем weight по правилам §5 — для desired ≤ 400
/// сначала меньшие, затем большие; для ≥ 600 — наоборот; 400 и 500 имеют
/// особый «swap» (400 пробует 500 первым, 500 — 400).
pub fn match_face<'a>(
    faces: &'a [FaceRecord],
    desired_weight: u16,
    desired_style: FontStyle,
) -> Option<&'a FaceRecord> {
    if faces.is_empty() {
        return None;
    }
    let min_style_pri = faces
        .iter()
        .map(|f| style_priority(f.style, desired_style))
        .min()?;
    faces
        .iter()
        .filter(|f| style_priority(f.style, desired_style) == min_style_pri)
        .min_by_key(|f| weight_priority(f.weight, desired_weight))
}

/// Приоритет face-style для заданного desired-style. Меньше — лучше.
/// Соответствует CSS Fonts L4 §5.2 (оригинал говорит про angle для oblique;
/// мы трактуем oblique как «не-italic, но наклонный»).
fn style_priority(face: FontStyle, desired: FontStyle) -> u8 {
    use FontStyle::*;
    match (desired, face) {
        (a, b) if a == b => 0,
        (Italic, Oblique) | (Oblique, Italic) => 1,
        (Italic, Normal) | (Oblique, Normal) => 2,
        (Normal, Oblique) => 1,
        (Normal, Italic) => 2,
        _ => 3,
    }
}

/// Приоритет face-weight для desired-weight. Меньше — лучше.
fn weight_priority(face: u16, desired: u16) -> (u32, u32) {
    if face == desired {
        return (0, 0);
    }
    match desired {
        400 => {
            if face == 500 {
                (1, 0)
            } else if face < 400 {
                (2, u32::from(400 - face))
            } else {
                // face > 500
                (3, u32::from(face - 400))
            }
        }
        500 => {
            if face == 400 {
                (1, 0)
            } else if face < 400 {
                (2, u32::from(400 - face))
            } else {
                // face > 500
                (3, u32::from(face - 500))
            }
        }
        d if d < 400 => {
            if face < d {
                (1, u32::from(d - face))
            } else {
                (2, u32::from(face - d))
            }
        }
        d => {
            if face > d {
                (1, u32::from(face - d))
            } else {
                (2, u32::from(d - face))
            }
        }
    }
}

#[cfg(test)]
mod font_provider_tests {
    use super::*;

    fn face(family: &str, weight: u16, style: FontStyle) -> FaceRecord {
        FaceRecord {
            family: family.to_string(),
            weight,
            style,
            path: PathBuf::from(format!("{family}-{weight}-{style:?}.ttf")),
        }
    }

    #[test]
    fn font_style_parse_keyword() {
        assert_eq!(FontStyle::parse_keyword("normal"), Some(FontStyle::Normal));
        assert_eq!(FontStyle::parse_keyword("ITALIC"), Some(FontStyle::Italic));
        assert_eq!(FontStyle::parse_keyword("Oblique"), Some(FontStyle::Oblique));
        assert_eq!(
            FontStyle::parse_keyword("oblique 20deg"),
            Some(FontStyle::Oblique)
        );
        assert_eq!(FontStyle::parse_keyword("bogus"), None);
        assert_eq!(FontStyle::parse_keyword(""), None);
    }

    #[test]
    fn match_face_exact_weight_and_style() {
        let faces = vec![
            face("Inter", 400, FontStyle::Normal),
            face("Inter", 700, FontStyle::Normal),
            face("Inter", 400, FontStyle::Italic),
        ];
        let m = match_face(&faces, 700, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 700);
        assert_eq!(m.style, FontStyle::Normal);
    }

    #[test]
    fn match_face_400_prefers_500_over_300() {
        // Spec: для desired=400 сначала пробуется 500, потом descending.
        let faces = vec![
            face("F", 300, FontStyle::Normal),
            face("F", 500, FontStyle::Normal),
        ];
        let m = match_face(&faces, 400, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 500);
    }

    #[test]
    fn match_face_500_prefers_400_over_600() {
        let faces = vec![
            face("F", 400, FontStyle::Normal),
            face("F", 600, FontStyle::Normal),
        ];
        let m = match_face(&faces, 500, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 400);
    }

    #[test]
    fn match_face_300_descends_then_ascends() {
        // desired=300 — сначала меньше desired, потом больше.
        // Кандидаты: 100 (-200), 500 (+200) → должен победить 100.
        let faces = vec![
            face("F", 100, FontStyle::Normal),
            face("F", 500, FontStyle::Normal),
        ];
        let m = match_face(&faces, 300, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 100);
    }

    #[test]
    fn match_face_700_ascends_then_descends() {
        // desired=700 — сначала больше desired, потом меньше.
        let faces = vec![
            face("F", 400, FontStyle::Normal),
            face("F", 900, FontStyle::Normal),
        ];
        let m = match_face(&faces, 700, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 900);
    }

    #[test]
    fn match_face_style_filter_strict_over_weight() {
        // Если запросили italic и italic есть с любым весом — он побеждает
        // даже более точный по весу normal.
        let faces = vec![
            face("F", 700, FontStyle::Normal),  // exact weight, but normal
            face("F", 100, FontStyle::Italic),  // wrong weight, but italic
        ];
        let m = match_face(&faces, 700, FontStyle::Italic).unwrap();
        assert_eq!(m.style, FontStyle::Italic);
        assert_eq!(m.weight, 100);
    }

    #[test]
    fn match_face_italic_falls_back_to_oblique_then_normal() {
        let faces_with_oblique = vec![
            face("F", 400, FontStyle::Normal),
            face("F", 400, FontStyle::Oblique),
        ];
        let m = match_face(&faces_with_oblique, 400, FontStyle::Italic).unwrap();
        assert_eq!(m.style, FontStyle::Oblique);

        let faces_normal_only = vec![face("F", 400, FontStyle::Normal)];
        let m = match_face(&faces_normal_only, 400, FontStyle::Italic).unwrap();
        assert_eq!(m.style, FontStyle::Normal);
    }

    #[test]
    fn match_face_normal_prefers_oblique_over_italic() {
        let faces = vec![
            face("F", 400, FontStyle::Italic),
            face("F", 400, FontStyle::Oblique),
        ];
        let m = match_face(&faces, 400, FontStyle::Normal).unwrap();
        assert_eq!(m.style, FontStyle::Oblique);
    }

    #[test]
    fn match_face_empty_returns_none() {
        let faces: Vec<FaceRecord> = Vec::new();
        assert!(match_face(&faces, 400, FontStyle::Normal).is_none());
    }

    #[test]
    fn match_face_full_css_weight_ladder_for_400() {
        // Order: 400, 500, 300, 200, 100, 600, 700, 800, 900.
        let weights = [100, 200, 300, 500, 600, 700, 800, 900];
        let mut faces: Vec<FaceRecord> =
            weights.iter().map(|&w| face("F", w, FontStyle::Normal)).collect();
        // 500 first
        let m = match_face(&faces, 400, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 500);
        // remove 500, expect 300
        faces.retain(|f| f.weight != 500);
        let m = match_face(&faces, 400, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 300);
        // remove 300/200/100, expect 600
        faces.retain(|f| f.weight > 500);
        let m = match_face(&faces, 400, FontStyle::Normal).unwrap();
        assert_eq!(m.weight, 600);
    }
}

/// JavaScript runtime — исполнение JS-кода (HTML inline scripts, `eval`,
/// custom elements, и т.д.). Trait абстрагирует выбор движка: первая
/// реализация — `rquickjs` поверх QuickJS (exception #4 в §5),
/// v1.0+ — `rusty_v8` поверх V8. Свой JS-движок не пишем.
///
/// Phase 0: trait определён, NullJsRuntime (всегда возвращает ошибку)
/// — placeholder. Реальный QuickJS-runtime появится в Phase 2-3.
///
/// Возвращаемые `JsValue`-ы — простые JSON-совместимые типы. Полный
/// JS-объект (с прототипами, методами, замыканиями) не пробрасывается
/// через границу trait-а — это намеренное ограничение, чтобы не привязать
/// API к конкретному движку.
pub trait JsRuntime: Send + Sync {
    /// Выполнить script-text и вернуть результат последнего выражения.
    fn eval(&self, script: &str) -> JsResult<JsValue>;

    /// Записать глобальную переменную в текущий runtime context.
    fn set_global(&self, name: &str, value: JsValue) -> JsResult<()>;

    /// Прочитать значение глобальной переменной.
    fn get_global(&self, name: &str) -> JsResult<JsValue>;

    /// Вызвать функцию `name(args)` в global scope и вернуть результат.
    /// Функция должна быть определена через `eval` или `set_global`.
    fn call_function(&self, name: &str, args: &[JsValue]) -> JsResult<JsValue>;

    /// Имя движка для debug-вывода. Реализация: `"quickjs"`, `"v8"`,
    /// `"null"`.
    fn engine_name(&self) -> &'static str;
}

/// Простые JSON-совместимые типы для передачи через trait-границу.
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Null,
    Undefined,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsValue>),
    /// Поля сортированы по ключу, чтобы сравнения детерминированы.
    Object(Vec<(String, JsValue)>),
}

impl JsValue {
    /// Хелпер: построить object из key-value пар.
    pub fn object<I: IntoIterator<Item = (String, JsValue)>>(entries: I) -> Self {
        let mut v: Vec<(String, JsValue)> = entries.into_iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        Self::Object(v)
    }
}

/// Ошибка исполнения JavaScript: либо syntax error (parse), либо runtime
/// exception (`throw`), либо неподдержанная операция (Null runtime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsError {
    /// Lexical / parser error в скрипте.
    Parse(String),
    /// Runtime error — uncaught exception в JS.
    Runtime(String),
    /// Операция не реализована в текущем runtime (например, Null).
    NotImplemented,
}

impl std::fmt::Display for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(m) => write!(f, "JS parse error: {m}"),
            Self::Runtime(m) => write!(f, "JS runtime error: {m}"),
            Self::NotImplemented => write!(f, "JS not implemented"),
        }
    }
}

impl std::error::Error for JsError {}

pub type JsResult<T> = std::result::Result<T, JsError>;

/// Null implementation — всегда возвращает `JsError::NotImplemented`.
/// Используется как placeholder в shell, пока не подключён реальный движок.
#[derive(Debug, Default)]
pub struct NullJsRuntime;

impl JsRuntime for NullJsRuntime {
    fn eval(&self, _: &str) -> JsResult<JsValue> {
        Err(JsError::NotImplemented)
    }
    fn set_global(&self, _: &str, _: JsValue) -> JsResult<()> {
        Err(JsError::NotImplemented)
    }
    fn get_global(&self, _: &str) -> JsResult<JsValue> {
        Err(JsError::NotImplemented)
    }
    fn call_function(&self, _: &str, _: &[JsValue]) -> JsResult<JsValue> {
        Err(JsError::NotImplemented)
    }
    fn engine_name(&self) -> &'static str {
        "null"
    }
}

// Точки расширения, спроектированные, но без интерфейса до Phase 1+.
//
// Trait-ы для четырёх «разрешённых exceptions» из §5 (внешние зависимости,
// которые мы используем): каждая зависимость прячется за свой trait,
// чтобы при желании можно было swap-нуть на свою реализацию.
//
// - WindowingBackend  — OS event loop + окна. Первая реализация: winit.
// - RenderBackend     — GPU-абстракция. Первая реализация: wgpu.
// - TlsBackend        — TLS / X.509 / симметричная криптография. Первая
//                       реализация: rustls. Своя — security antipattern;
//                       абстракция нужна только для swap на системный TLS
//                       (SChannel / Network.framework).
// - JsRuntime         — определён выше; реализации: QuickJS (v0.5),
//                       V8 (v1.0+).
//
// Остальные точки расширения без выбранной зависимости — пишем свои
// реализации сразу:
//
// - HyphenationEngine — переносы слов для CSS hyphens. Phase 2.
// - DnsResolver       — определён выше; реализации: SystemDnsResolver
//                       (через `(host, port).to_socket_addrs()`),
//                       CachedDnsResolver (обёртка с `lumen-storage::DnsCache`).
//                       DoH/DoT-резолверы — Phase 2+.
// - Hasher            — единый интерфейс хэшей (для CSP, SRI). Phase 1.

#[cfg(test)]
mod tests {
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
}
