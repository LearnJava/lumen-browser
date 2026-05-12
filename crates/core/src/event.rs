//! События, которые модули и плагины могут наблюдать.
//!
//! Это «словарь» событий, не сама шина. Шину (EventBus) реализуем позже,
//! когда появится первый потребитель за пределами одного процесса.

use crate::url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u32);

#[derive(Debug, Clone)]
pub enum Event {
    TabCreated { tab_id: TabId },
    TabClosed { tab_id: TabId },
    Navigation { tab_id: TabId, url: Url },
    PageLoaded { tab_id: TabId, url: Url },
    RequestStarted { tab_id: TabId, url: Url },
    RequestCompleted { tab_id: TabId, url: Url, status: u16 },
    RequestBlocked { tab_id: TabId, url: Url, reason: String },
}
