//! События, которые модули и плагины могут наблюдать.
//!
//! Это «словарь» событий, не сама шина. Шину (EventBus) реализуем позже,
//! когда появится первый потребитель за пределами одного процесса.

use crate::url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u32);

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
}
