//! UX-CLOSE-WARNINGS: предупреждение при закрытии окна — только о том, что
//! нельзя вернуть.
//!
//! Обычные вкладки восстанавливаются сессией, поэтому «Закрыть N вкладок?»
//! не показывается никогда. Спрашиваем о двух вещах: идущие загрузки (их
//! прервёт выход) и вкладки эфемерного анонимного профиля (после выхода их
//! содержимого нет нигде). `beforeunload` активной вкладки обрабатывает
//! отдельный шлюз ([`crate::lumen`], `beforeunload_gate`) и тут не считается.
//! Модель чистая — счётчики приходят параметрами, правила тестируются без окна.

/// Что потеряется при выходе, если пользователь согласится.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CloseLoss {
    /// Загрузки в состоянии `Pending`/`InProgress`.
    pub(crate) downloads: usize,
    /// Открытые вкладки анонимного профиля (0, если профиль другой).
    pub(crate) anonymous_tabs: usize,
}

impl CloseLoss {
    /// `anonymous_profile` — активен ли анонимный профиль; тогда в потерю
    /// входят все `tabs` окна.
    pub(crate) fn new(downloads: usize, anonymous_profile: bool, tabs: usize) -> Self {
        Self { downloads, anonymous_tabs: if anonymous_profile { tabs } else { 0 } }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.downloads == 0 && self.anonymous_tabs == 0
    }

    /// Текст диалога или `None`, если терять нечего.
    pub(crate) fn message(self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if self.downloads > 0 {
            parts.push(format!("Загрузок в процессе: {} — выход прервёт их.", self.downloads));
        }
        if self.anonymous_tabs > 0 {
            parts.push(format!(
                "Вкладок анонимного профиля: {} — после выхода их не восстановить.",
                self.anonymous_tabs
            ));
        }
        Some(parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_ordinary_tabs_do_not_warn() {
        // Приёмка: 20 обычных вкладок — закрытие без вопроса.
        assert!(CloseLoss::new(0, false, 20).message().is_none());
    }

    #[test]
    fn download_in_flight_warns_with_count() {
        let m = CloseLoss::new(1, false, 20).message().unwrap();
        assert!(m.contains("Загрузок в процессе: 1"), "{m}");
        let m = CloseLoss::new(3, false, 1).message().unwrap();
        assert!(m.contains("Загрузок в процессе: 3"), "{m}");
    }

    #[test]
    fn anonymous_profile_counts_every_tab() {
        let loss = CloseLoss::new(0, true, 4);
        assert_eq!(loss.anonymous_tabs, 4);
        let m = loss.message().unwrap();
        assert!(m.contains("анонимного профиля: 4"), "{m}");
        assert!(!m.contains("загруз"), "{m}");
    }

    #[test]
    fn both_losses_are_listed() {
        let m = CloseLoss::new(2, true, 1).message().unwrap();
        assert!(m.contains("процессе: 2") && m.contains("профиля: 1"), "{m}");
    }
}
