//! CORS preflight + response validation по Fetch Standard §3.2.2 — §4.10.
//! <https://fetch.spec.whatwg.org/#http-cors-protocol>
//!
//! Идея CORS: same-origin policy запрещает JS-коду читать cross-origin
//! ответы. CORS — opt-in механизм, через который сервер на cross-origin
//! ресурсе сообщает «я разрешаю чтение моих ответов из такого-то origin-а».
//! Реализуется через специальные `Access-Control-*` заголовки.
//!
//! Этот модуль — **классификатор и спецификация**, не enforcer. Он
//! решает:
//! - является ли запрос «CORS-safelisted» (можно отправлять без preflight,
//!   но ответ всё равно проверяется);
//! - нужен ли preflight (OPTIONS перед actual request);
//! - какие заголовки нужны на preflight-запросе;
//! - что должно быть в preflight-ответе, чтобы actual request разрешить;
//! - что должно быть в actual response, чтобы ответ вернуть caller-у;
//! - кеширование preflight-результатов по `Access-Control-Max-Age`.
//!
//! Реальная отправка OPTIONS + повторная отправка actual request + хранение
//! кеша между запросами интегрируется в `HttpClient` отдельной задачей
//! (см. roadmap «CORS preflight» в `lumen-plan.md`). Этот модуль остаётся
//! pure-функцией от входа — что облегчает unit-тестирование без mock-сервера.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lumen_core::url::Url;

use crate::origin::Origin;

/// Credentials mode по Fetch §3.1 — определяет, прикладывать ли cookies /
/// HTTP auth к cross-origin запросу и проверять ли `Access-Control-Allow-Credentials`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CredentialsMode {
    /// «omit» — никаких credentials, ACAC игнорируется.
    Omit,
    /// «same-origin» — credentials только same-origin. Cross-origin запросы
    /// ведут себя как Omit на этапе CORS-проверки.
    #[default]
    SameOrigin,
    /// «include» — credentials прикладываются всегда; ACAC=true обязателен,
    /// ACAO=`*` недопустим.
    Include,
}

impl CredentialsMode {
    /// Применяются ли credentials для cross-origin запроса в этом режиме?
    /// SameOrigin для cross-origin запроса = no credentials.
    pub fn cross_origin_credentials(self) -> bool {
        matches!(self, Self::Include)
    }
}

/// Cross-origin запрос — описание для решения о preflight и сборки CORS-заголовков.
///
/// `headers` — author-set заголовки запроса (то, что user code хочет послать).
/// `forbidden request-header names` (Fetch §4.4.4) caller обязан отфильтровать
/// заранее: эти заголовки JS не имеет права ставить, и в `headers` они не
/// должны попасть. Это позволяет нам не различать «JS-set» vs «UA-set» здесь.
#[derive(Debug, Clone)]
pub struct CorsRequest {
    pub origin: Origin,
    pub target: Url,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub credentials_mode: CredentialsMode,
}

// ── CORS-safelisted предикаты (Fetch §4.4) ───────────────────────────────────

/// «CORS-safelisted method» (Fetch §4.4.1): GET / HEAD / POST.
/// Метод не-safelisted (PUT/DELETE/PATCH/…) сам по себе требует preflight.
pub fn is_cors_safelisted_method(method: &str) -> bool {
    let m = method.trim();
    m.eq_ignore_ascii_case("GET") || m.eq_ignore_ascii_case("HEAD") || m.eq_ignore_ascii_case("POST")
}

/// «forbidden request-header name» (Fetch §4.4.4). UA-controlled заголовки,
/// которые JS не имеет права устанавливать — соответственно, попасть в
/// `CorsRequest.headers` они не должны. Список — для filter-проверки caller-а
/// (например, при mapping-е author-headers перед построением `CorsRequest`).
pub fn is_forbidden_request_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Точные имена.
    if matches!(
        lower.as_str(),
        "accept-charset"
            | "accept-encoding"
            | "access-control-request-headers"
            | "access-control-request-method"
            | "connection"
            | "content-length"
            | "cookie"
            | "cookie2"
            | "date"
            | "dnt"
            | "expect"
            | "host"
            | "keep-alive"
            | "origin"
            | "referer"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "via"
    ) {
        return true;
    }
    // Префиксы — `Sec-*` и `Proxy-*` (Fetch §4.4.4 шаг 2-3).
    lower.starts_with("sec-") || lower.starts_with("proxy-")
}

/// «CORS-safelisted request-header» (Fetch §4.4.2). Возвращает true, если
/// заголовок (имя+значение) — один из шести «безопасных» и не требует
/// включения в `Access-Control-Request-Headers` preflight-а.
///
/// Также spec ограничивает длину значения 128 байт суммарно по safelisted
/// заголовкам — это проверка caller-а на этапе сбора `CorsRequest`;
/// здесь мы про неё не знаем (стейтлесс per-header check), но в
/// `unsafe_request_header_names` учтём.
pub fn is_cors_safelisted_request_header(name: &str, value: &str) -> bool {
    if value.len() > MAX_SAFELISTED_HEADER_VALUE_LEN {
        return false;
    }
    if value
        .bytes()
        .any(|b| b == 0x00 || (b < 0x20 && b != b'\t') || b == 0x7F)
    {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "accept" | "accept-language" | "content-language" => true,
        "content-type" => is_cors_safelisted_content_type(value),
        // `Range` safelisted, только если value — simple byte range из одного
        // диапазона `bytes=START-END` либо `bytes=START-` (Fetch §4.4.2 шаг 7).
        // Suffix-форма `bytes=-N` и multi-range НЕ safelisted.
        "range" => is_cors_safelisted_range_value(value),
        _ => false,
    }
}

/// Лимит длины значения «safelisted» заголовка (Fetch §4.4.2 step 3).
pub const MAX_SAFELISTED_HEADER_VALUE_LEN: usize = 128;

/// «CORS-safelisted Content-Type» (Fetch §4.4.2): одна из трёх MIME-форм
/// без чувствительности к регистру. Парсер берёт essence — часть до `;`
/// (если есть), trim-ит whitespace, сравнивает.
pub fn is_cors_safelisted_content_type(value: &str) -> bool {
    let essence = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
    )
}

/// «simple byte range» для CORS-safelisted Range header (Fetch §4.4.2 шаг 7):
/// строго `bytes=START-END` или `bytes=START-`, где START и END — десятичные
/// без знака, START ≤ END. Suffix `bytes=-N` НЕ safelisted (требует preflight).
fn is_cors_safelisted_range_value(value: &str) -> bool {
    let rest = match value.strip_prefix("bytes=") {
        Some(r) => r,
        None => return false,
    };
    let (start_s, end_s) = match rest.split_once('-') {
        Some(p) => p,
        None => return false,
    };
    let start: u64 = match start_s.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if end_s.is_empty() {
        return true; // bytes=START-
    }
    match end_s.parse::<u64>() {
        Ok(end) => start <= end,
        Err(_) => false,
    }
}

// ── Решение «нужен preflight» (Fetch §4.8 step 1) ────────────────────────────

/// Возвращает true, если запрос требует preflight перед actual request.
/// Условие (Fetch §4.8 step 1):
/// - method не CORS-safelisted, ИЛИ
/// - есть хоть один request-header вне CORS-safelisted (с учётом значения).
///
/// Forbidden headers сюда не считаются: caller обязан был их отфильтровать
/// до построения `CorsRequest`. Если их случайно оставили — они тоже сделают
/// «нужен preflight», что не нарушит безопасности (хуже, чем нужно — но
/// не лучше).
///
/// Также Fetch §4.8 учитывает credentials mode для wildcard-семантики
/// ответа, но не для **необходимости** preflight — поэтому здесь mode
/// не участвует.
pub fn needs_preflight(req: &CorsRequest) -> bool {
    if !is_cors_safelisted_method(&req.method) {
        return true;
    }
    req.headers
        .iter()
        .any(|(name, value)| !is_cors_safelisted_request_header(name, value))
}

/// Имена «unsafe» author-заголовков (lowercased + sorted lexicographically)
/// для значения `Access-Control-Request-Headers` preflight-а.
///
/// Включает все имена, что **не** прошли safelisted-проверку. Сортировка по
/// lexicographic ASCII order (требование Fetch §4.8 step 7.1 — «sorted byte
/// sequence»).
///
/// Дубликаты имён схлопываются в одно (case-insensitively).
pub fn unsafe_request_header_names(headers: &[(String, String)]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (name, value) in headers {
        if !is_cors_safelisted_request_header(name, value) {
            let lower = name.to_ascii_lowercase();
            if !names.contains(&lower) {
                names.push(lower);
            }
        }
    }
    names.sort();
    names
}

// ── Сборка preflight-запроса (Fetch §4.8 steps 3-7) ──────────────────────────

/// Заголовки OPTIONS preflight-запроса.
///
/// Spec §4.8 step 2: метод = OPTIONS, URL = same as actual request.
/// Заголовки:
/// - `Origin: <serialize(origin)>` (HTTP Semantics, RFC 6454 §7);
/// - `Access-Control-Request-Method: <actual method>` (uppercased);
/// - `Access-Control-Request-Headers: <comma-separated sorted lowercased
///   unsafe header names>` (если их хотя бы один).
///
/// Возвращает `Vec<(name, value)>` в формате, готовом к подстановке в HTTP
/// request line. Caller сам выставляет `Host`, `Content-Length: 0`,
/// `Connection`, etc. — это уровень HttpClient.
pub fn build_preflight_headers(req: &CorsRequest) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(4);
    out.push(("Origin".to_string(), req.origin.serialize()));
    out.push((
        "Access-Control-Request-Method".to_string(),
        req.method.to_ascii_uppercase(),
    ));
    let unsafe_headers = unsafe_request_header_names(&req.headers);
    if !unsafe_headers.is_empty() {
        out.push((
            "Access-Control-Request-Headers".to_string(),
            unsafe_headers.join(","),
        ));
    }
    out
}

// ── Парсинг preflight-ответа ─────────────────────────────────────────────────

/// Результат успешного preflight-а. Кешируется по (origin, target_origin,
/// credentials_mode) на `max_age_seconds`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightResult {
    /// Список allowed methods (lowercased). Wildcard `*` сохраняется как
    /// единственный элемент `"*"` (см. [`PreflightResult::method_allowed`]).
    /// При credentials_mode=Include wildcard не допускается, и сюда попадает
    /// только explicit-список.
    pub allowed_methods: Vec<String>,
    /// Список allowed headers (lowercased). Wildcard `*` — также элемент
    /// `"*"`. Authorization wildcard НЕ покрывает (Fetch §4.10) — но это
    /// проверяется в [`PreflightResult::headers_allowed`], а не на этапе
    /// парсинга.
    pub allowed_headers: Vec<String>,
    /// `Access-Control-Allow-Credentials: true` присутствовал.
    pub allow_credentials: bool,
    /// TTL в секундах для кеша preflight-а. Если ACMaxAge отсутствует —
    /// default 5 секунд (Fetch §4.8 «default cache duration»).
    pub max_age_seconds: u32,
}

impl PreflightResult {
    /// Покрывает ли результат preflight-а метод `method` (case-insensitive)?
    pub fn method_allowed(&self, method: &str) -> bool {
        let lower = method.to_ascii_lowercase();
        if self.allowed_methods.iter().any(|m| m == "*") {
            return true;
        }
        // CORS-safelisted methods неявно разрешены, даже если не в списке
        // ACAM (Fetch §4.10 шаг 5 — actual method match включает safelisted
        // даже без явного перечисления).
        if is_cors_safelisted_method(method) {
            return true;
        }
        self.allowed_methods.iter().any(|m| m == &lower)
    }

    /// Покрывает ли результат preflight-а все unsafe-заголовки запроса?
    /// `request_headers` — author-set, в любом регистре. Возвращает имя
    /// первого отвергнутого заголовка или None если всё ок.
    ///
    /// Authorization не покрывается wildcard-ом — должен быть явно перечислен.
    pub fn unmatched_header(&self, request_headers: &[(String, String)]) -> Option<String> {
        let has_wildcard = self.allowed_headers.iter().any(|h| h == "*");
        for (name, value) in request_headers {
            if is_cors_safelisted_request_header(name, value) {
                continue;
            }
            let lower = name.to_ascii_lowercase();
            if has_wildcard && lower != "authorization" {
                continue;
            }
            if self.allowed_headers.iter().any(|h| h == &lower) {
                continue;
            }
            return Some(lower);
        }
        None
    }
}

/// Ошибки CORS-валидации (preflight или actual response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsError {
    /// preflight-ответ имеет статус вне 200-299.
    PreflightStatusNotOk(u16),
    /// `Access-Control-Allow-Origin` отсутствует в ответе.
    AllowOriginMissing,
    /// ACAO задаёт другой origin / не совпадает с requestor.
    AllowOriginMismatch {
        expected: String,
        got: String,
    },
    /// ACAO=`*`, но credentials_mode=Include — wildcard не допустим.
    WildcardOriginWithCredentials,
    /// credentials_mode=Include, но ответ не вернул `Access-Control-Allow-Credentials: true`.
    CredentialsNotAllowed,
    /// Actual method отсутствует в `Access-Control-Allow-Methods` и не CORS-safelisted.
    MethodNotAllowed(String),
    /// Заголовок author-request отсутствует в `Access-Control-Allow-Headers`.
    HeaderNotAllowed(String),
    /// `Access-Control-Max-Age` непарсимый — нечисловой / переполнение.
    MaxAgeInvalid(String),
}

impl std::fmt::Display for CorsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreflightStatusNotOk(s) => write!(f, "preflight status not 200-299: {s}"),
            Self::AllowOriginMissing => f.write_str("Access-Control-Allow-Origin missing"),
            Self::AllowOriginMismatch { expected, got } => write!(
                f,
                "Access-Control-Allow-Origin mismatch: expected {expected}, got {got}"
            ),
            Self::WildcardOriginWithCredentials => f.write_str(
                "Access-Control-Allow-Origin=* incompatible with credentials mode=include",
            ),
            Self::CredentialsNotAllowed => {
                f.write_str("Access-Control-Allow-Credentials != true while credentials=include")
            }
            Self::MethodNotAllowed(m) => write!(f, "method not in Access-Control-Allow-Methods: {m}"),
            Self::HeaderNotAllowed(h) => write!(f, "header not in Access-Control-Allow-Headers: {h}"),
            Self::MaxAgeInvalid(v) => write!(f, "Access-Control-Max-Age invalid: {v}"),
        }
    }
}

impl std::error::Error for CorsError {}

/// Default cache duration по Fetch §4.8 (когда ACMaxAge не задан).
pub const DEFAULT_PREFLIGHT_MAX_AGE_SECONDS: u32 = 5;

/// Полный разбор preflight-ответа. Возвращает [`PreflightResult`] для
/// кеша или ошибку.
///
/// Алгоритм по Fetch §4.8 steps 9-15 + §4.10 для ACAO/ACAC:
/// 1. Status ∈ 200..=299.
/// 2. `Access-Control-Allow-Origin` валидирован против `req.origin` и
///    `req.credentials_mode` (см. [`validate_allow_origin`]).
/// 3. `Access-Control-Allow-Credentials` парсится при `credentials_mode=Include`.
/// 4. `Access-Control-Allow-Methods` — comma-separated список (lowercased),
///    либо `*`.
/// 5. `Access-Control-Allow-Headers` — то же.
/// 6. `Access-Control-Max-Age` — десятичное число секунд; default = 5 если
///    отсутствует; парсинг-ошибка → [`CorsError::MaxAgeInvalid`].
pub fn evaluate_preflight_response(
    status: u16,
    response_headers: &[(String, String)],
    req: &CorsRequest,
) -> Result<PreflightResult, CorsError> {
    if !(200..=299).contains(&status) {
        return Err(CorsError::PreflightStatusNotOk(status));
    }
    let allow_origin_wildcard =
        validate_allow_origin(response_headers, &req.origin, req.credentials_mode)?;
    let allow_credentials =
        validate_allow_credentials(response_headers, req.credentials_mode)?;
    let allowed_methods = parse_comma_list(response_headers, "access-control-allow-methods");
    // Если ACAM = `*` и credentials_mode = Include — wildcard не действует
    // как «всё», но Fetch §4.10 шаг 5 говорит «match по lowercased токену».
    // Мы сохраняем элемент `"*"` и в [`PreflightResult::method_allowed`]
    // проверяем contextually.
    let _ = allow_origin_wildcard;
    let allowed_methods = restrict_wildcard_for_credentials(allowed_methods, req.credentials_mode);
    let allowed_headers = parse_comma_list(response_headers, "access-control-allow-headers");
    let allowed_headers = restrict_wildcard_for_credentials(allowed_headers, req.credentials_mode);

    // Шаг 5+6 (Fetch §4.8): actual method и actual headers должны быть в
    // соответствующих Allow-списках уже на этапе preflight-а.
    let provisional = PreflightResult {
        allowed_methods,
        allowed_headers,
        allow_credentials,
        max_age_seconds: parse_max_age(response_headers)?,
    };
    if !provisional.method_allowed(&req.method) {
        return Err(CorsError::MethodNotAllowed(req.method.clone()));
    }
    if let Some(h) = provisional.unmatched_header(&req.headers) {
        return Err(CorsError::HeaderNotAllowed(h));
    }
    Ok(provisional)
}

/// Валидация ACAO + ACAC на **actual response** (не preflight) — Fetch §4.10
/// шаги 1-4. Не проверяет ACAM / ACAH — это работа preflight-кеша.
///
/// Возвращает Ok(true) если ACAO был wildcard, Ok(false) для explicit-match.
pub fn check_cors_response_headers(
    response_headers: &[(String, String)],
    request_origin: &Origin,
    credentials_mode: CredentialsMode,
) -> Result<bool, CorsError> {
    let wildcard = validate_allow_origin(response_headers, request_origin, credentials_mode)?;
    validate_allow_credentials(response_headers, credentials_mode)?;
    Ok(wildcard)
}

fn validate_allow_origin(
    response_headers: &[(String, String)],
    request_origin: &Origin,
    credentials_mode: CredentialsMode,
) -> Result<bool, CorsError> {
    let value = match header_value(response_headers, "access-control-allow-origin") {
        Some(v) => v,
        None => return Err(CorsError::AllowOriginMissing),
    };
    let trimmed = value.trim();
    if trimmed == "*" {
        if matches!(credentials_mode, CredentialsMode::Include) {
            return Err(CorsError::WildcardOriginWithCredentials);
        }
        return Ok(true);
    }
    let expected = request_origin.serialize();
    if trimmed.eq_ignore_ascii_case(&expected) {
        return Ok(false);
    }
    Err(CorsError::AllowOriginMismatch {
        expected,
        got: trimmed.to_string(),
    })
}

fn validate_allow_credentials(
    response_headers: &[(String, String)],
    credentials_mode: CredentialsMode,
) -> Result<bool, CorsError> {
    let value = header_value(response_headers, "access-control-allow-credentials");
    let is_true = value.map(|v| v.trim().eq_ignore_ascii_case("true")).unwrap_or(false);
    if matches!(credentials_mode, CredentialsMode::Include) && !is_true {
        return Err(CorsError::CredentialsNotAllowed);
    }
    Ok(is_true)
}

fn parse_max_age(response_headers: &[(String, String)]) -> Result<u32, CorsError> {
    let value = match header_value(response_headers, "access-control-max-age") {
        Some(v) => v.trim().to_string(),
        None => return Ok(DEFAULT_PREFLIGHT_MAX_AGE_SECONDS),
    };
    value
        .parse::<u32>()
        .map_err(|_| CorsError::MaxAgeInvalid(value.clone()))
}

fn parse_comma_list(response_headers: &[(String, String)], name: &str) -> Vec<String> {
    let raw = match header_value(response_headers, name) {
        Some(v) => v,
        None => return Vec::new(),
    };
    raw.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn restrict_wildcard_for_credentials(
    list: Vec<String>,
    credentials_mode: CredentialsMode,
) -> Vec<String> {
    if !matches!(credentials_mode, CredentialsMode::Include) {
        return list;
    }
    // Wildcard `*` в credentials=include режиме не действует как `everything`;
    // Fetch §4.10 предписывает literal-match. Просто оставляем `*` как
    // explicit-token: он совпадёт только с буквальным «*»-методом / -header-ом,
    // которых не бывает. Фактически — wildcard «исчезает» для credentials.
    list.into_iter().filter(|s| s != "*").collect()
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    for (k, v) in headers {
        if k.eq_ignore_ascii_case(name) {
            return Some(v.as_str());
        }
    }
    None
}

// ── Кеш preflight-результатов ────────────────────────────────────────────────

/// Кеш preflight-результатов по `(requestor_origin, target_origin,
/// credentials_mode)`. TTL — `max_age_seconds` из preflight-ответа.
///
/// **Thread-safe** через `Mutex` — у нас HttpClient может разделяться между
/// потоками shell-а (см. shell streaming pipeline). Hot path коротким захватом
/// в `lookup` / `insert` под мьютекс.
///
/// API:
/// - [`PreflightCache::insert`] кладёт результат под current `now`.
/// - [`PreflightCache::lookup`] возвращает entry, если не истёк.
/// - [`PreflightCache::allows`] — высокоуровневая «можно ли пропустить
///   preflight для этого запроса» (учитывает method + headers + TTL).
#[derive(Debug, Default)]
pub struct PreflightCache {
    entries: Mutex<HashMap<CacheKey, CacheEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    requestor: Origin,
    target: Origin,
    credentials_mode: CredentialsMode,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    result: PreflightResult,
    expires_at: Duration,
}

impl PreflightCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Добавить результат preflight-а в кеш. `now` — текущее время от UNIX
    /// epoch (используется системное в `insert_now` ниже, явное `now` —
    /// для тестов с управляемым клоком).
    pub fn insert_at(
        &self,
        requestor: Origin,
        target: Origin,
        credentials_mode: CredentialsMode,
        result: PreflightResult,
        now: Duration,
    ) {
        let key = CacheKey {
            requestor,
            target,
            credentials_mode,
        };
        let expires_at = now + Duration::from_secs(u64::from(result.max_age_seconds));
        let entry = CacheEntry { result, expires_at };
        if let Ok(mut map) = self.entries.lock() {
            map.insert(key, entry);
        }
    }

    /// То же что [`Self::insert_at`], но с `now = SystemTime::now()`. Для
    /// продакшн-пути.
    pub fn insert(
        &self,
        requestor: Origin,
        target: Origin,
        credentials_mode: CredentialsMode,
        result: PreflightResult,
    ) {
        self.insert_at(requestor, target, credentials_mode, result, now());
    }

    /// Достать НЕИСТЁКШЕЕ entry. Истёкшие удаляются lazy (next-access
    /// очистка через `take_if`-like логику).
    pub fn lookup_at(
        &self,
        requestor: &Origin,
        target: &Origin,
        credentials_mode: CredentialsMode,
        now: Duration,
    ) -> Option<PreflightResult> {
        let mut map = self.entries.lock().ok()?;
        let key = CacheKey {
            requestor: requestor.clone(),
            target: target.clone(),
            credentials_mode,
        };
        let entry = map.get(&key)?;
        if entry.expires_at <= now {
            map.remove(&key);
            return None;
        }
        Some(entry.result.clone())
    }

    pub fn lookup(
        &self,
        requestor: &Origin,
        target: &Origin,
        credentials_mode: CredentialsMode,
    ) -> Option<PreflightResult> {
        self.lookup_at(requestor, target, credentials_mode, now())
    }

    /// Возвращает true, если кеш содержит подходящее entry для `req` (метод
    /// и заголовки покрыты, не истекло). Безопасный shortcut для caller-а
    /// уровня HttpClient: при true — preflight можно не отправлять.
    pub fn allows_at(&self, req: &CorsRequest, now: Duration) -> bool {
        let target = match Origin::from_url(&req.target) {
            Ok(o) => o,
            Err(_) => return false,
        };
        let entry = match self.lookup_at(&req.origin, &target, req.credentials_mode, now) {
            Some(e) => e,
            None => return false,
        };
        if !entry.method_allowed(&req.method) {
            return false;
        }
        entry.unmatched_header(&req.headers).is_none()
    }

    pub fn allows(&self, req: &CorsRequest) -> bool {
        self.allows_at(req, now())
    }

    /// Полная очистка (для тестов / Profile switching).
    pub fn clear(&self) {
        if let Ok(mut map) = self.entries.lock() {
            map.clear();
        }
    }
}

fn now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(s: &str) -> Origin {
        Origin::from_url(&Url::parse(s).unwrap()).unwrap()
    }

    fn req(method: &str, target: &str, headers: &[(&str, &str)]) -> CorsRequest {
        CorsRequest {
            origin: origin("https://app.example.com/"),
            target: Url::parse(target).unwrap(),
            method: method.to_string(),
            headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            credentials_mode: CredentialsMode::SameOrigin,
        }
    }

    // ── safelisted method ────────────────────────────────────────────────────

    #[test]
    fn get_head_post_are_safelisted_methods() {
        assert!(is_cors_safelisted_method("GET"));
        assert!(is_cors_safelisted_method("HEAD"));
        assert!(is_cors_safelisted_method("POST"));
        assert!(is_cors_safelisted_method("get")); // case-insensitive
        assert!(is_cors_safelisted_method("  POST  ")); // trimmed
    }

    #[test]
    fn put_patch_delete_not_safelisted() {
        assert!(!is_cors_safelisted_method("PUT"));
        assert!(!is_cors_safelisted_method("PATCH"));
        assert!(!is_cors_safelisted_method("DELETE"));
        assert!(!is_cors_safelisted_method("OPTIONS"));
        assert!(!is_cors_safelisted_method("CONNECT"));
        assert!(!is_cors_safelisted_method("TRACE"));
    }

    // ── forbidden headers ────────────────────────────────────────────────────

    #[test]
    fn forbidden_exact_names() {
        for name in [
            "Accept-Charset",
            "accept-encoding",
            "Access-Control-Request-Method",
            "Access-Control-Request-Headers",
            "Connection",
            "Content-Length",
            "Cookie",
            "Cookie2",
            "Date",
            "DNT",
            "Expect",
            "Host",
            "Keep-Alive",
            "Origin",
            "Referer",
            "TE",
            "Trailer",
            "Transfer-Encoding",
            "Upgrade",
            "Via",
        ] {
            assert!(
                is_forbidden_request_header(name),
                "{name} should be forbidden"
            );
        }
    }

    #[test]
    fn forbidden_prefixes() {
        assert!(is_forbidden_request_header("Sec-Fetch-Site"));
        assert!(is_forbidden_request_header("sec-anything"));
        assert!(is_forbidden_request_header("Proxy-Authorization"));
        assert!(is_forbidden_request_header("proxy-anything"));
    }

    #[test]
    fn normal_headers_not_forbidden() {
        assert!(!is_forbidden_request_header("Authorization"));
        assert!(!is_forbidden_request_header("X-Custom"));
        assert!(!is_forbidden_request_header("Accept"));
    }

    // ── safelisted request headers ───────────────────────────────────────────

    #[test]
    fn accept_language_safelisted_value_ok() {
        assert!(is_cors_safelisted_request_header("Accept-Language", "en-US"));
        assert!(is_cors_safelisted_request_header("accept-language", "ru,en;q=0.8"));
    }

    #[test]
    fn content_language_safelisted() {
        assert!(is_cors_safelisted_request_header("Content-Language", "ru"));
    }

    #[test]
    fn content_type_simple_forms_safelisted() {
        assert!(is_cors_safelisted_request_header(
            "Content-Type",
            "application/x-www-form-urlencoded"
        ));
        assert!(is_cors_safelisted_request_header(
            "content-type",
            "multipart/form-data; boundary=---xxx"
        ));
        assert!(is_cors_safelisted_request_header("Content-Type", "text/plain"));
        assert!(is_cors_safelisted_request_header(
            "Content-Type",
            "Text/Plain; charset=utf-8"
        ));
    }

    #[test]
    fn content_type_json_not_safelisted() {
        assert!(!is_cors_safelisted_request_header("Content-Type", "application/json"));
        assert!(!is_cors_safelisted_request_header("Content-Type", "application/xml"));
    }

    #[test]
    fn value_longer_than_128_not_safelisted() {
        let long = "a".repeat(129);
        assert!(!is_cors_safelisted_request_header("Accept-Language", &long));
    }

    #[test]
    fn value_with_control_chars_not_safelisted() {
        assert!(!is_cors_safelisted_request_header("Accept", "text/plain\r\nX: y"));
        assert!(!is_cors_safelisted_request_header("Accept", "text/plain\0"));
    }

    #[test]
    fn unknown_header_not_safelisted() {
        assert!(!is_cors_safelisted_request_header("X-Custom", "1"));
        assert!(!is_cors_safelisted_request_header("Authorization", "Bearer x"));
    }

    #[test]
    fn range_simple_byte_range_safelisted() {
        assert!(is_cors_safelisted_request_header("Range", "bytes=0-1023"));
        assert!(is_cors_safelisted_request_header("Range", "bytes=1024-"));
    }

    #[test]
    fn range_suffix_not_safelisted() {
        // bytes=-N не покрыт CORS-safelisted (Fetch §4.4.2 шаг 7).
        assert!(!is_cors_safelisted_request_header("Range", "bytes=-512"));
    }

    #[test]
    fn range_with_bad_start_end_not_safelisted() {
        assert!(!is_cors_safelisted_request_header("Range", "bytes=10-5")); // end < start
        assert!(!is_cors_safelisted_request_header("Range", "bytes=abc-1"));
        assert!(!is_cors_safelisted_request_header("Range", "items=0-1"));
    }

    // ── needs_preflight ──────────────────────────────────────────────────────

    #[test]
    fn simple_get_no_preflight() {
        assert!(!needs_preflight(&req("GET", "https://api.example.org/", &[])));
    }

    #[test]
    fn simple_post_form_urlencoded_no_preflight() {
        assert!(!needs_preflight(&req(
            "POST",
            "https://api.example.org/",
            &[("Content-Type", "application/x-www-form-urlencoded")],
        )));
    }

    #[test]
    fn post_json_needs_preflight() {
        assert!(needs_preflight(&req(
            "POST",
            "https://api.example.org/",
            &[("Content-Type", "application/json")],
        )));
    }

    #[test]
    fn put_needs_preflight() {
        assert!(needs_preflight(&req("PUT", "https://api.example.org/", &[])));
    }

    #[test]
    fn delete_needs_preflight() {
        assert!(needs_preflight(&req("DELETE", "https://api.example.org/", &[])));
    }

    #[test]
    fn custom_header_needs_preflight() {
        assert!(needs_preflight(&req(
            "GET",
            "https://api.example.org/",
            &[("X-Auth", "token")],
        )));
    }

    #[test]
    fn authorization_header_needs_preflight() {
        // Authorization не в safelisted — preflight нужен.
        assert!(needs_preflight(&req(
            "GET",
            "https://api.example.org/",
            &[("Authorization", "Bearer x")],
        )));
    }

    #[test]
    fn safelisted_value_over_128_chars_needs_preflight() {
        // Per-header лимит — 128 байт (Fetch §4.4.2 шаг 1). Длинное значение
        // safelisted-имени превращает его в non-safelisted → preflight.
        let long = "a".repeat(129);
        assert!(needs_preflight(&req(
            "GET",
            "https://api.example.org/",
            &[("Accept-Language", &long)],
        )));
    }

    // ── unsafe_request_header_names ──────────────────────────────────────────

    #[test]
    fn unsafe_names_lowercased_and_sorted() {
        let headers = vec![
            ("X-Foo".into(), "1".into()),
            ("Authorization".into(), "Bearer x".into()),
            ("A-Custom".into(), "v".into()),
        ];
        assert_eq!(
            unsafe_request_header_names(&headers),
            vec!["a-custom".to_string(), "authorization".into(), "x-foo".into()]
        );
    }

    #[test]
    fn safelisted_headers_skipped() {
        let headers = vec![
            ("Accept".into(), "text/html".into()),
            ("X-Foo".into(), "1".into()),
        ];
        assert_eq!(unsafe_request_header_names(&headers), vec!["x-foo".to_string()]);
    }

    #[test]
    fn duplicate_names_deduplicated() {
        let headers = vec![
            ("X-Foo".into(), "1".into()),
            ("x-foo".into(), "2".into()),
        ];
        assert_eq!(unsafe_request_header_names(&headers), vec!["x-foo".to_string()]);
    }

    // ── build_preflight_headers ──────────────────────────────────────────────

    #[test]
    fn preflight_minimum_origin_and_method() {
        let r = req("DELETE", "https://api.example.org/u/42", &[]);
        let pre = build_preflight_headers(&r);
        assert_eq!(
            pre,
            vec![
                ("Origin".into(), "https://app.example.com".into()),
                ("Access-Control-Request-Method".into(), "DELETE".into()),
            ]
        );
    }

    #[test]
    fn preflight_includes_request_headers() {
        let r = req(
            "PUT",
            "https://api.example.org/",
            &[("X-Foo", "1"), ("Authorization", "Bearer x")],
        );
        let pre = build_preflight_headers(&r);
        assert_eq!(
            pre[2],
            (
                "Access-Control-Request-Headers".into(),
                "authorization,x-foo".into(),
            )
        );
    }

    #[test]
    fn preflight_method_uppercased() {
        let r = req("patch", "https://api.example.org/", &[]);
        let pre = build_preflight_headers(&r);
        assert_eq!(pre[1].1, "PATCH");
    }

    // ── evaluate_preflight_response ──────────────────────────────────────────

    fn pre_response(headers: &[(&str, &str)]) -> Vec<(String, String)> {
        headers
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn preflight_status_must_be_2xx() {
        let r = req("PUT", "https://api.example.org/", &[]);
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
        ]);
        assert!(matches!(
            evaluate_preflight_response(404, &resp, &r),
            Err(CorsError::PreflightStatusNotOk(404))
        ));
        assert!(matches!(
            evaluate_preflight_response(500, &resp, &r),
            Err(CorsError::PreflightStatusNotOk(500))
        ));
        assert!(evaluate_preflight_response(204, &resp, &r).is_ok());
    }

    #[test]
    fn preflight_allow_origin_missing() {
        let r = req("PUT", "https://api.example.org/", &[]);
        let resp = pre_response(&[("Access-Control-Allow-Methods", "PUT")]);
        assert_eq!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::AllowOriginMissing)
        );
    }

    #[test]
    fn preflight_allow_origin_wildcard_ok_for_omit() {
        let mut r = req("PUT", "https://api.example.org/", &[]);
        r.credentials_mode = CredentialsMode::Omit;
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "*"),
            ("Access-Control-Allow-Methods", "PUT"),
        ]);
        let res = evaluate_preflight_response(200, &resp, &r).unwrap();
        assert_eq!(res.allowed_methods, vec!["put".to_string()]);
        assert!(!res.allow_credentials);
    }

    #[test]
    fn preflight_allow_origin_wildcard_forbidden_with_credentials() {
        let mut r = req("PUT", "https://api.example.org/", &[]);
        r.credentials_mode = CredentialsMode::Include;
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "*"),
            ("Access-Control-Allow-Methods", "PUT"),
            ("Access-Control-Allow-Credentials", "true"),
        ]);
        assert_eq!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::WildcardOriginWithCredentials)
        );
    }

    #[test]
    fn preflight_allow_origin_explicit_must_match_exact() {
        let r = req("PUT", "https://api.example.org/", &[]);
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://other.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
        ]);
        assert!(matches!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::AllowOriginMismatch { .. })
        ));
    }

    #[test]
    fn preflight_credentials_include_requires_acac_true() {
        let mut r = req("PUT", "https://api.example.org/", &[]);
        r.credentials_mode = CredentialsMode::Include;
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
        ]);
        assert_eq!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::CredentialsNotAllowed)
        );
    }

    #[test]
    fn preflight_method_must_be_in_allow_methods() {
        let r = req("PUT", "https://api.example.org/", &[]);
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "DELETE,PATCH"),
        ]);
        assert!(matches!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::MethodNotAllowed(_))
        ));
    }

    #[test]
    fn preflight_safelisted_method_allowed_even_without_explicit() {
        // GET в safelisted — preflight для него не отправлен бы; но если
        // вдруг отправлен (например, из-за custom header), ACAM может не
        // упомянуть GET — он всё равно match-нет.
        let r = req(
            "GET",
            "https://api.example.org/",
            &[("X-Custom", "1")],
        );
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
            ("Access-Control-Allow-Headers", "x-custom"),
        ]);
        assert!(evaluate_preflight_response(200, &resp, &r).is_ok());
    }

    #[test]
    fn preflight_wildcard_method_covers_any_method() {
        let mut r = req("PURGE", "https://api.example.org/", &[]);
        r.credentials_mode = CredentialsMode::Omit;
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "*"),
            ("Access-Control-Allow-Methods", "*"),
        ]);
        assert!(evaluate_preflight_response(200, &resp, &r).is_ok());
    }

    #[test]
    fn preflight_wildcard_method_does_not_apply_with_credentials() {
        let mut r = req("PURGE", "https://api.example.org/", &[]);
        r.credentials_mode = CredentialsMode::Include;
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "*"),
            ("Access-Control-Allow-Credentials", "true"),
        ]);
        // PURGE не safelisted, wildcard в credentials=include не действует,
        // явного PURGE в списке нет → MethodNotAllowed.
        assert!(matches!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::MethodNotAllowed(_))
        ));
    }

    #[test]
    fn preflight_headers_must_be_in_allow_headers() {
        let r = req(
            "PUT",
            "https://api.example.org/",
            &[("X-Auth", "v")],
        );
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
            ("Access-Control-Allow-Headers", "x-other"),
        ]);
        assert_eq!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::HeaderNotAllowed("x-auth".into()))
        );
    }

    #[test]
    fn preflight_wildcard_headers_cover_custom_but_not_authorization() {
        let mut r = req(
            "PUT",
            "https://api.example.org/",
            &[("X-Foo", "1")],
        );
        r.credentials_mode = CredentialsMode::Omit;
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "*"),
            ("Access-Control-Allow-Methods", "PUT"),
            ("Access-Control-Allow-Headers", "*"),
        ]);
        assert!(evaluate_preflight_response(200, &resp, &r).is_ok());

        // Authorization не покрыт wildcard-ом.
        r.headers.push(("Authorization".into(), "Bearer x".into()));
        assert_eq!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::HeaderNotAllowed("authorization".into()))
        );
    }

    #[test]
    fn preflight_max_age_parsed() {
        let r = req("PUT", "https://api.example.org/", &[]);
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
            ("Access-Control-Max-Age", "600"),
        ]);
        let res = evaluate_preflight_response(200, &resp, &r).unwrap();
        assert_eq!(res.max_age_seconds, 600);
    }

    #[test]
    fn preflight_max_age_default_when_missing() {
        let r = req("PUT", "https://api.example.org/", &[]);
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
        ]);
        let res = evaluate_preflight_response(200, &resp, &r).unwrap();
        assert_eq!(res.max_age_seconds, DEFAULT_PREFLIGHT_MAX_AGE_SECONDS);
    }

    #[test]
    fn preflight_max_age_invalid_errors() {
        let r = req("PUT", "https://api.example.org/", &[]);
        let resp = pre_response(&[
            ("Access-Control-Allow-Origin", "https://app.example.com"),
            ("Access-Control-Allow-Methods", "PUT"),
            ("Access-Control-Max-Age", "later"),
        ]);
        assert!(matches!(
            evaluate_preflight_response(200, &resp, &r),
            Err(CorsError::MaxAgeInvalid(_))
        ));
    }

    // ── check_cors_response_headers (actual response) ────────────────────────

    #[test]
    fn actual_response_ok_with_explicit_origin() {
        let o = origin("https://app.example.com/");
        let resp = pre_response(&[("Access-Control-Allow-Origin", "https://app.example.com")]);
        assert_eq!(
            check_cors_response_headers(&resp, &o, CredentialsMode::SameOrigin),
            Ok(false)
        );
    }

    #[test]
    fn actual_response_ok_with_wildcard() {
        let o = origin("https://app.example.com/");
        let resp = pre_response(&[("Access-Control-Allow-Origin", "*")]);
        assert_eq!(
            check_cors_response_headers(&resp, &o, CredentialsMode::Omit),
            Ok(true)
        );
    }

    #[test]
    fn actual_response_missing_acao_errors() {
        let o = origin("https://app.example.com/");
        let resp = pre_response(&[]);
        assert_eq!(
            check_cors_response_headers(&resp, &o, CredentialsMode::SameOrigin),
            Err(CorsError::AllowOriginMissing),
        );
    }

    // ── PreflightCache ───────────────────────────────────────────────────────

    fn make_result(methods: &[&str], headers: &[&str], max_age: u32) -> PreflightResult {
        PreflightResult {
            allowed_methods: methods.iter().map(|s| (*s).to_string()).collect(),
            allowed_headers: headers.iter().map(|s| (*s).to_string()).collect(),
            allow_credentials: false,
            max_age_seconds: max_age,
        }
    }

    #[test]
    fn cache_insert_lookup_returns_entry() {
        let cache = PreflightCache::new();
        let requestor = origin("https://app.example.com/");
        let target = origin("https://api.example.org/");
        let res = make_result(&["put"], &["x-foo"], 60);
        cache.insert_at(
            requestor.clone(),
            target.clone(),
            CredentialsMode::SameOrigin,
            res.clone(),
            Duration::from_secs(0),
        );
        let got = cache
            .lookup_at(
                &requestor,
                &target,
                CredentialsMode::SameOrigin,
                Duration::from_secs(30),
            )
            .unwrap();
        assert_eq!(got, res);
    }

    #[test]
    fn cache_expired_entry_returns_none() {
        let cache = PreflightCache::new();
        let requestor = origin("https://app.example.com/");
        let target = origin("https://api.example.org/");
        let res = make_result(&["put"], &[], 60);
        cache.insert_at(
            requestor.clone(),
            target.clone(),
            CredentialsMode::SameOrigin,
            res,
            Duration::from_secs(0),
        );
        assert!(
            cache
                .lookup_at(
                    &requestor,
                    &target,
                    CredentialsMode::SameOrigin,
                    Duration::from_secs(60),
                )
                .is_none()
        );
        assert!(
            cache
                .lookup_at(
                    &requestor,
                    &target,
                    CredentialsMode::SameOrigin,
                    Duration::from_secs(60_000),
                )
                .is_none()
        );
    }

    #[test]
    fn cache_credentials_mode_keys_independently() {
        let cache = PreflightCache::new();
        let requestor = origin("https://app.example.com/");
        let target = origin("https://api.example.org/");
        let res = make_result(&["put"], &[], 60);
        cache.insert_at(
            requestor.clone(),
            target.clone(),
            CredentialsMode::SameOrigin,
            res,
            Duration::from_secs(0),
        );
        assert!(
            cache
                .lookup_at(
                    &requestor,
                    &target,
                    CredentialsMode::Include,
                    Duration::from_secs(10),
                )
                .is_none()
        );
    }

    #[test]
    fn cache_allows_short_circuits_preflight() {
        let cache = PreflightCache::new();
        let r = req(
            "PUT",
            "https://api.example.org/",
            &[("X-Foo", "v")],
        );
        let target = Origin::from_url(&r.target).unwrap();
        let res = make_result(&["put"], &["x-foo"], 60);
        cache.insert_at(
            r.origin.clone(),
            target.clone(),
            CredentialsMode::SameOrigin,
            res,
            Duration::from_secs(0),
        );
        assert!(cache.allows_at(&r, Duration::from_secs(30)));
    }

    #[test]
    fn cache_allows_false_when_method_not_covered() {
        let cache = PreflightCache::new();
        let r = req(
            "DELETE",
            "https://api.example.org/",
            &[],
        );
        let target = Origin::from_url(&r.target).unwrap();
        let res = make_result(&["put"], &[], 60);
        cache.insert_at(
            r.origin.clone(),
            target.clone(),
            CredentialsMode::SameOrigin,
            res,
            Duration::from_secs(0),
        );
        assert!(!cache.allows_at(&r, Duration::from_secs(30)));
    }

    #[test]
    fn cache_allows_false_when_header_not_covered() {
        let cache = PreflightCache::new();
        let r = req(
            "PUT",
            "https://api.example.org/",
            &[("X-Foo", "v"), ("X-Bar", "v2")],
        );
        let target = Origin::from_url(&r.target).unwrap();
        let res = make_result(&["put"], &["x-foo"], 60);
        cache.insert_at(
            r.origin.clone(),
            target.clone(),
            CredentialsMode::SameOrigin,
            res,
            Duration::from_secs(0),
        );
        assert!(!cache.allows_at(&r, Duration::from_secs(30)));
    }

    #[test]
    fn cache_clear_empties_state() {
        let cache = PreflightCache::new();
        let r = req("PUT", "https://api.example.org/", &[]);
        let target = Origin::from_url(&r.target).unwrap();
        let res = make_result(&["put"], &[], 60);
        cache.insert_at(
            r.origin.clone(),
            target.clone(),
            CredentialsMode::SameOrigin,
            res,
            Duration::from_secs(0),
        );
        assert!(cache.allows_at(&r, Duration::from_secs(30)));
        cache.clear();
        assert!(!cache.allows_at(&r, Duration::from_secs(30)));
    }

    // ── CredentialsMode helpers ──────────────────────────────────────────────

    #[test]
    fn credentials_mode_cross_origin_credentials() {
        assert!(!CredentialsMode::Omit.cross_origin_credentials());
        assert!(!CredentialsMode::SameOrigin.cross_origin_credentials());
        assert!(CredentialsMode::Include.cross_origin_credentials());
    }

    #[test]
    fn credentials_mode_default_is_same_origin() {
        assert_eq!(CredentialsMode::default(), CredentialsMode::SameOrigin);
    }
}
