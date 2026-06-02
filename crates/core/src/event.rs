//! События, которые модули и плагины могут наблюдать.
//!
//! Это «словарь» событий, не сама шина. Шину (EventBus) реализуем позже,
//! когда появится первый потребитель за пределами одного процесса.

use crate::url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u32);

/// Стадия сетевого запроса, на которой произошёл сбой.
///
/// Используется в [`Event::RequestFailed`], чтобы наблюдатель (network log UI)
/// мог дать пользователю осмысленное объяснение вместо общего «не удалось
/// загрузить»: на каком именно этапе соединение споткнулось. Порядок вариантов
/// соответствует последовательности установки соединения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestStage {
    /// Резолв hostname → IP не удался (NXDOMAIN, таймаут DNS, пустой ответ).
    /// User-facing: «не удалось найти сервер».
    Dns,
    /// TCP-соединение к резолвленному адресу не установилось
    /// (connection refused, сеть недоступна, таймаут connect).
    /// User-facing: «не удалось подключиться к серверу».
    Tcp,
    /// TLS-рукопожатие провалилось (недействительный сертификат, несовпадение
    /// hostname, ошибка ALPN, неподдерживаемый протокол).
    /// User-facing: «защищённое соединение не установлено».
    Tls,
    /// Соединение установлено, но обмен HTTP-данными прервался
    /// (EOF до статуса, ошибка чтения тела, битый chunked-стрим, ошибка записи).
    /// User-facing: «соединение прервано во время загрузки».
    Read,
}

impl RequestStage {
    /// Машинно-читаемый тег стадии для логов и сериализации (`"dns"`/`"tcp"`/
    /// `"tls"`/`"read"`). Не локализуется — это идентификатор, не UI-строка.
    pub fn as_str(self) -> &'static str {
        match self {
            RequestStage::Dns => "dns",
            RequestStage::Tcp => "tcp",
            RequestStage::Tls => "tls",
            RequestStage::Read => "read",
        }
    }
}

/// Тип subresource-ресурса, найденного preload-сканером.
/// Используется в [`Event::SubresourceHintFound`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubresourceKind {
    Stylesheet,
    Script,
    Image,
    Font,
    Preconnect { dns_only: bool },
    Other { as_kind: Option<String> },
}

/// Приоритет выборки subresource-а. Отражает HTML Living Standard §17.2.3
/// «Priority» и Fetch Standard §2.2 «request priority».
///
/// Числовое значение (`as u8`) используется для сортировки: High < Medium < Low
/// (меньшее число = более приоритетный).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FetchPriority {
    /// Блокируют рендер или критичны для first paint:
    /// CSS-файлы, шрифты, `<link rel="preconnect">`.
    High = 0,
    /// Полезны, но не блокируют рендер: скрипты без defer/async.
    Medium = 1,
    /// Не критичны для first paint: изображения, srcset, generic preload.
    Low = 2,
}

impl FetchPriority {
    /// Приоритет по типу subresource (Fetch Standard §2.2).
    pub fn for_kind(kind: &SubresourceKind) -> Self {
        match kind {
            SubresourceKind::Stylesheet
            | SubresourceKind::Font
            | SubresourceKind::Preconnect { .. } => FetchPriority::High,
            SubresourceKind::Script => FetchPriority::Medium,
            SubresourceKind::Image | SubresourceKind::Other { .. } => FetchPriority::Low,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    TabCreated { tab_id: TabId },
    TabClosed { tab_id: TabId },
    Navigation { tab_id: TabId, url: Url },
    PageLoaded { tab_id: TabId, url: Url },
    RequestStarted { tab_id: TabId, url: Url },
    RequestCompleted { tab_id: TabId, url: Url, status: u16 },
    RequestBlocked { tab_id: TabId, url: Url, reason: String },
    /// Запрос не дошёл до ответа: сбой произошёл на сетевом уровне (DNS / TCP /
    /// TLS / чтение) **до** получения HTTP-статуса. Делает явным инвариант
    /// «RequestStarted без RequestCompleted = сбой»: ровно один из
    /// `RequestCompleted` / `RequestFailed` / `RequestBlocked` следует за
    /// каждым `RequestStarted`. `stage` локализует точку отказа для UI,
    /// `reason` — техническое сообщение (`Error::Network`-текст) для network log.
    RequestFailed { tab_id: TabId, url: Url, stage: RequestStage, reason: String },
    /// Preload-сканер обнаружил subresource-ссылку до DOM-парсинга
    /// (HTML LS §13.2.6.4.7). `url` — абсолютный URL (резолвится 4B.3).
    /// `priority` — рекомендованный fetch-приоритет (Fetch Standard §2.2).
    SubresourceHintFound {
        url: String,
        kind: SubresourceKind,
        priority: FetchPriority,
    },
    /// RFC 6455 §1.3: handshake завершён, соединение открыто.
    WebSocketConnected { tab_id: TabId, url: Url },
    /// Получено сообщение от сервера (текст или бинарные данные).
    WebSocketMessage { tab_id: TabId, url: Url, is_binary: bool, len: usize },
    /// Соединение закрыто (либо нормально, либо по ошибке).
    WebSocketClosed { tab_id: TabId, url: Url, code: Option<u16>, reason: String },
    /// Ошибка транспортного слоя (до или после открытия).
    WebSocketError { tab_id: TabId, url: Url, message: String },
    /// HTML Living Standard §9.2: SSE-соединение установлено (200 OK, text/event-stream).
    SseConnected { tab_id: TabId, url: Url },
    /// Получено SSE-событие от сервера.
    SseMessage { tab_id: TabId, url: Url, event_type: String, data: String, id: Option<String> },
    /// SSE-соединение закрыто (штатно или по ошибке транспорта).
    SseClosed { tab_id: TabId, url: Url, reason: String },
    /// Транспортная ошибка SSE (до/после connect, до reconnect).
    SseError { tab_id: TabId, url: Url, message: String },
    // ── IME composition events (UI Events Specification §5.3) ────────────────
    /// Начало IME-composition сессии. Диспатчится при `Ime::Enabled`
    /// или при первом `Preedit` с непустым текстом.
    /// JS: `compositionstart` с `data = ""`.
    ImeCompositionStarted { tab_id: TabId },
    /// Preedit обновился (Ime::Preedit с непустым текстом).
    /// JS: `compositionupdate` с `data = preedit_text`.
    ImeCompositionUpdated { tab_id: TabId, data: String },
    /// Финальный символ зафиксирован (Ime::Commit).
    /// JS: `compositionend` с `data = committed_text`.
    ImeCompositionEnded { tab_id: TabId, data: String },
    /// HTML LS §form-submission: пользователь нажал submit-кнопку, валидация
    /// прошла. Для GET-форм `body` нужно добавить к `action` как query-строку;
    /// для POST — отправить как тело запроса с Content-Type urlencoded.
    FormSubmit {
        tab_id: TabId,
        /// Целевой URL формы (значение атрибута `action`; пустая строка если
        /// атрибут отсутствует — навигация к текущей странице).
        action: String,
        /// HTTP-метод: `"get"` или `"post"` (нижний регистр).
        method: String,
        /// Сериализованные данные формы (application/x-www-form-urlencoded).
        body: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_events_are_clone() {
        let e = Event::ImeCompositionStarted { tab_id: TabId(1) };
        let _ = e.clone();
        let e2 = Event::ImeCompositionUpdated { tab_id: TabId(1), data: "あ".to_string() };
        let _ = e2.clone();
        let e3 = Event::ImeCompositionEnded { tab_id: TabId(1), data: "あい".to_string() };
        let _ = e3.clone();
    }

    #[test]
    fn ime_event_debug() {
        let e = Event::ImeCompositionUpdated { tab_id: TabId(0), data: "test".into() };
        assert!(format!("{e:?}").contains("ImeCompositionUpdated"));
    }
}
