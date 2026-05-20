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
    /// (HTML LS §13.2.6.4.7). `url` — сырая строка из атрибута (`href`/`src`),
    /// ещё не разрешённая относительно base (это делает 4B.3).
    SubresourceHintFound { url: String, kind: SubresourceKind },
}
