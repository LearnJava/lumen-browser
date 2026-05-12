//! Capability-модель для плагинов и внутренних модулей.
//!
//! Плагин не имеет доступа к ресурсам по умолчанию. Чтобы получить доступ —
//! запрашивает Capability, пользователь решает (см. §11.4 плана).

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Сетевые запросы. Whitelist доменов, на которые можно ходить.
    Network { domains: Vec<String> },
    /// Чтение/запись в собственный namespace плагина.
    Storage,
    /// Чтение/запись буфера обмена.
    Clipboard,
    /// Рисование UI в сайдбаре.
    UiSidebar,
    /// Подписка на события указанных категорий.
    EventStream { categories: Vec<String> },
    /// Регистрация команд в команд-палитре.
    CommandPalette,
    /// Чтение выделенного текста на странице.
    SelectionRead,
    /// Модификация выделенного текста.
    SelectionWrite,
}

#[derive(Debug, Clone)]
pub struct CapabilityToken {
    pub plugin_id: String,
    pub capability: Capability,
    /// `None` — действует, пока пользователь не отзовёт.
    pub expires_at: Option<u64>,
}
