//! `<iframe sandbox>` flags по HTML Living Standard §7.6.5.
//! <https://html.spec.whatwg.org/multipage/origin.html#sandboxing-flag-set>
//!
//! Семантика sandbox-а инвертирующая: `<iframe>` без `sandbox` атрибута —
//! без ограничений; `<iframe sandbox>` — **максимальные** ограничения,
//! и каждый перечисленный keyword **снимает** одно из них.
//!
//! Эта модель отражена в типе [`SandboxFlags`] как bitset из «активных
//! ограничений». Парсер заводит `SandboxFlags::all_restrictions()` и
//! очищает биты по найденным `allow-*` keyword-ам.
//!
//! Применение sandbox flags (создать opaque origin, заблокировать формы /
//! скрипты / popup-ы, …) — задача shell-я / DOM-загрузчика; этот модуль
//! даёт только парсер и точное состояние ограничений.

/// Битовое поле sandbox-ограничений. Конкретный бит == «**запрет** этой
/// способности активен». Соответствует sandboxing flag set из HTML LS §7.6.5.
///
/// 18 флагов; ниже комментарии повторяют ключевую строку spec-а, чтобы
/// при добавлении нового CSS/HTML feature был понятен импликейшен.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SandboxFlags(u32);

impl SandboxFlags {
    /// «navigation browsing context flag» — запрет навигации top-level и
    /// auxiliary browsing context, кроме user activation.
    pub const NAVIGATION: Self = Self(1 << 0);
    /// «auxiliary navigation browsing context flag» — запрет открытия новых
    /// auxiliary browsing context-ов (popup-ов). Управляется `allow-popups`.
    pub const AUXILIARY_NAVIGATION: Self = Self(1 << 1);
    /// «top-level navigation without user activation» — `allow-top-navigation`.
    pub const TOP_NAVIGATION: Self = Self(1 << 2);
    /// «top-level navigation with user activation» — `allow-top-navigation-by-user-activation`.
    pub const TOP_NAVIGATION_USER_ACTIVATION: Self = Self(1 << 3);
    /// `allow-top-navigation-to-custom-protocols`.
    pub const TOP_NAVIGATION_CUSTOM_PROTOCOLS: Self = Self(1 << 4);
    /// «forced opaque origin» — `allow-same-origin` (когда снят — origin
    /// nested документа opaque, не равный родителю).
    pub const ORIGIN: Self = Self(1 << 5);
    /// «forms» — `allow-forms`.
    pub const FORMS: Self = Self(1 << 6);
    /// «pointer lock» — `allow-pointer-lock`.
    pub const POINTER_LOCK: Self = Self(1 << 7);
    /// «scripts» — `allow-scripts`.
    pub const SCRIPTS: Self = Self(1 << 8);
    /// «automatic features» (autoplay, autofocus) — `allow-popups-to-escape-sandbox`
    /// и `allow-scripts` вместе влияют на это; spec выделяет отдельный flag.
    pub const AUTOMATIC_FEATURES: Self = Self(1 << 9);
    /// «storage access by user activation» — `allow-storage-access-by-user-activation`.
    pub const STORAGE_ACCESS_BY_USER_ACTIVATION: Self = Self(1 << 10);
    /// «document.domain setter» — всегда применяется в sandbox-е.
    pub const DOCUMENT_DOMAIN: Self = Self(1 << 11);
    /// «WebSocket» — sandboxed document не открывает WebSocket без `allow-scripts`.
    pub const WEBSOCKET: Self = Self(1 << 12);
    /// «propagates to auxiliary» — `allow-popups-to-escape-sandbox` снимает это.
    pub const PROPAGATES_TO_AUXILIARY: Self = Self(1 << 13);
    /// «modals» (alert/confirm/prompt/print) — `allow-modals`.
    pub const MODALS: Self = Self(1 << 14);
    /// «orientation lock» — `allow-orientation-lock`.
    pub const ORIENTATION_LOCK: Self = Self(1 << 15);
    /// «presentation» — `allow-presentation`.
    pub const PRESENTATION: Self = Self(1 << 16);
    /// «downloads» — `allow-downloads`.
    pub const DOWNLOADS: Self = Self(1 << 17);

    /// Пустой набор — sandbox не активен (без ограничений).
    pub fn empty() -> Self {
        Self(0)
    }

    /// Все ограничения активны — стартовое состояние для `<iframe sandbox>`
    /// с пустым атрибутом. allow-keyword-ы снимают биты с этого набора.
    pub fn all_restrictions() -> Self {
        Self(
            Self::NAVIGATION.0
                | Self::AUXILIARY_NAVIGATION.0
                | Self::TOP_NAVIGATION.0
                | Self::TOP_NAVIGATION_USER_ACTIVATION.0
                | Self::TOP_NAVIGATION_CUSTOM_PROTOCOLS.0
                | Self::ORIGIN.0
                | Self::FORMS.0
                | Self::POINTER_LOCK.0
                | Self::SCRIPTS.0
                | Self::AUTOMATIC_FEATURES.0
                | Self::STORAGE_ACCESS_BY_USER_ACTIVATION.0
                | Self::DOCUMENT_DOMAIN.0
                | Self::WEBSOCKET.0
                | Self::PROPAGATES_TO_AUXILIARY.0
                | Self::MODALS.0
                | Self::ORIENTATION_LOCK.0
                | Self::PRESENTATION.0
                | Self::DOWNLOADS.0,
        )
    }

    /// `true` если **все** биты из `other` установлены в `self` —
    /// «активирован ли запрет».
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// `true` если ни один бит не установлен (sandbox = пустой набор
    /// ограничений = `<iframe>` без атрибута).
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Снять биты `other` из `self` — используется парсером для `allow-*`.
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// Добавить биты `other`.
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Удобство для тестов / shell-а: получить сырой битсет.
    pub fn bits(self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for SandboxFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for SandboxFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// Парсит значение HTML атрибута `sandbox` в [`SandboxFlags`].
///
/// Семантика по HTML LS §«sandboxing flag set» / §«parse a sandboxing
/// directive»:
/// - всегда стартуем с `all_restrictions()`;
/// - токенизация — ASCII whitespace splitter, токены case-insensitive;
/// - каждый известный `allow-*` keyword **снимает** соответствующий бит;
/// - неизвестные токены игнорируются (forward-compatible).
///
/// Если значение `None` (атрибут отсутствует) — возвращаем `empty()`
/// (sandbox не активен, без ограничений). Если значение `Some("")`
/// (атрибут есть без value) — возвращаем `all_restrictions()`.
pub fn parse_sandbox_value(value: Option<&str>) -> SandboxFlags {
    let Some(value) = value else {
        return SandboxFlags::empty();
    };
    let mut flags = SandboxFlags::all_restrictions();
    for token in value.split_ascii_whitespace() {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "allow-popups" => flags.remove(SandboxFlags::AUXILIARY_NAVIGATION),
            "allow-top-navigation" => {
                flags.remove(SandboxFlags::TOP_NAVIGATION);
                flags.remove(SandboxFlags::TOP_NAVIGATION_USER_ACTIVATION);
                flags.remove(SandboxFlags::TOP_NAVIGATION_CUSTOM_PROTOCOLS);
            }
            "allow-top-navigation-by-user-activation" => {
                flags.remove(SandboxFlags::TOP_NAVIGATION_USER_ACTIVATION);
            }
            "allow-top-navigation-to-custom-protocols" => {
                flags.remove(SandboxFlags::TOP_NAVIGATION_CUSTOM_PROTOCOLS);
            }
            "allow-same-origin" => flags.remove(SandboxFlags::ORIGIN),
            "allow-forms" => flags.remove(SandboxFlags::FORMS),
            "allow-pointer-lock" => flags.remove(SandboxFlags::POINTER_LOCK),
            "allow-scripts" => {
                flags.remove(SandboxFlags::SCRIPTS);
                // По §«parse a sandboxing directive»: allow-scripts также
                // снимает automatic features (autofocus и т.д.).
                flags.remove(SandboxFlags::AUTOMATIC_FEATURES);
            }
            "allow-popups-to-escape-sandbox" => {
                flags.remove(SandboxFlags::PROPAGATES_TO_AUXILIARY);
            }
            "allow-modals" => flags.remove(SandboxFlags::MODALS),
            "allow-orientation-lock" => flags.remove(SandboxFlags::ORIENTATION_LOCK),
            "allow-presentation" => flags.remove(SandboxFlags::PRESENTATION),
            "allow-downloads" => flags.remove(SandboxFlags::DOWNLOADS),
            "allow-storage-access-by-user-activation" => {
                flags.remove(SandboxFlags::STORAGE_ACCESS_BY_USER_ACTIVATION);
            }
            _ => {}
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_attribute_means_no_restrictions() {
        let f = parse_sandbox_value(None);
        assert!(f.is_empty());
    }

    #[test]
    fn empty_attribute_means_all_restrictions() {
        let f = parse_sandbox_value(Some(""));
        assert_eq!(f, SandboxFlags::all_restrictions());
        assert!(f.contains(SandboxFlags::SCRIPTS));
        assert!(f.contains(SandboxFlags::ORIGIN));
        assert!(f.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn whitespace_only_attribute_is_like_empty() {
        let f = parse_sandbox_value(Some("   \t\n "));
        assert_eq!(f, SandboxFlags::all_restrictions());
    }

    #[test]
    fn allow_scripts_lifts_scripts_and_automatic_features() {
        let f = parse_sandbox_value(Some("allow-scripts"));
        assert!(!f.contains(SandboxFlags::SCRIPTS));
        assert!(!f.contains(SandboxFlags::AUTOMATIC_FEATURES));
        assert!(f.contains(SandboxFlags::ORIGIN));
        assert!(f.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn allow_same_origin_lifts_origin_only() {
        let f = parse_sandbox_value(Some("allow-same-origin"));
        assert!(!f.contains(SandboxFlags::ORIGIN));
        assert!(f.contains(SandboxFlags::SCRIPTS));
        assert!(f.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn allow_forms_lifts_forms_only() {
        let f = parse_sandbox_value(Some("allow-forms"));
        assert!(!f.contains(SandboxFlags::FORMS));
        assert!(f.contains(SandboxFlags::SCRIPTS));
    }

    #[test]
    fn multiple_keywords_combine() {
        let f = parse_sandbox_value(Some("allow-scripts allow-same-origin"));
        assert!(!f.contains(SandboxFlags::SCRIPTS));
        assert!(!f.contains(SandboxFlags::ORIGIN));
        assert!(f.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn unknown_keywords_ignored() {
        let f = parse_sandbox_value(Some("allow-scripts allow-magic"));
        assert!(!f.contains(SandboxFlags::SCRIPTS));
        assert!(f.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn keyword_case_insensitive() {
        let f = parse_sandbox_value(Some("Allow-Scripts ALLOW-FORMS"));
        assert!(!f.contains(SandboxFlags::SCRIPTS));
        assert!(!f.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn whitespace_tokenization_per_html_ls() {
        let f = parse_sandbox_value(Some("allow-scripts\tallow-forms\nallow-popups"));
        assert!(!f.contains(SandboxFlags::SCRIPTS));
        assert!(!f.contains(SandboxFlags::FORMS));
        assert!(!f.contains(SandboxFlags::AUXILIARY_NAVIGATION));
    }

    #[test]
    fn allow_top_navigation_implies_subset_keywords() {
        let f = parse_sandbox_value(Some("allow-top-navigation"));
        assert!(!f.contains(SandboxFlags::TOP_NAVIGATION));
        assert!(!f.contains(SandboxFlags::TOP_NAVIGATION_USER_ACTIVATION));
        assert!(!f.contains(SandboxFlags::TOP_NAVIGATION_CUSTOM_PROTOCOLS));
    }

    #[test]
    fn allow_top_navigation_user_activation_is_narrower() {
        let f = parse_sandbox_value(Some("allow-top-navigation-by-user-activation"));
        assert!(f.contains(SandboxFlags::TOP_NAVIGATION));
        assert!(!f.contains(SandboxFlags::TOP_NAVIGATION_USER_ACTIVATION));
    }

    #[test]
    fn allow_popups() {
        let f = parse_sandbox_value(Some("allow-popups"));
        assert!(!f.contains(SandboxFlags::AUXILIARY_NAVIGATION));
        assert!(f.contains(SandboxFlags::SCRIPTS));
    }

    #[test]
    fn allow_modals() {
        let f = parse_sandbox_value(Some("allow-modals"));
        assert!(!f.contains(SandboxFlags::MODALS));
    }

    #[test]
    fn allow_downloads() {
        let f = parse_sandbox_value(Some("allow-downloads"));
        assert!(!f.contains(SandboxFlags::DOWNLOADS));
    }

    #[test]
    fn allow_pointer_lock() {
        let f = parse_sandbox_value(Some("allow-pointer-lock"));
        assert!(!f.contains(SandboxFlags::POINTER_LOCK));
    }

    #[test]
    fn allow_presentation_and_orientation_lock() {
        let f = parse_sandbox_value(Some("allow-presentation allow-orientation-lock"));
        assert!(!f.contains(SandboxFlags::PRESENTATION));
        assert!(!f.contains(SandboxFlags::ORIENTATION_LOCK));
    }

    #[test]
    fn allow_storage_access_by_user_activation() {
        let f = parse_sandbox_value(Some("allow-storage-access-by-user-activation"));
        assert!(!f.contains(SandboxFlags::STORAGE_ACCESS_BY_USER_ACTIVATION));
    }

    #[test]
    fn allow_popups_to_escape_sandbox() {
        let f = parse_sandbox_value(Some("allow-popups-to-escape-sandbox"));
        assert!(!f.contains(SandboxFlags::PROPAGATES_TO_AUXILIARY));
    }

    #[test]
    fn document_domain_always_active() {
        let f = parse_sandbox_value(Some(
            "allow-scripts allow-same-origin allow-forms allow-popups \
             allow-top-navigation allow-pointer-lock allow-modals \
             allow-orientation-lock allow-presentation allow-downloads \
             allow-popups-to-escape-sandbox \
             allow-storage-access-by-user-activation",
        ));
        assert!(f.contains(SandboxFlags::DOCUMENT_DOMAIN));
    }

    #[test]
    fn bit_or_and_bitand() {
        let f = SandboxFlags::SCRIPTS | SandboxFlags::FORMS;
        assert!(f.contains(SandboxFlags::SCRIPTS));
        assert!(f.contains(SandboxFlags::FORMS));
        let g = f & SandboxFlags::SCRIPTS;
        assert_eq!(g, SandboxFlags::SCRIPTS);
    }

    #[test]
    fn duplicate_keywords_idempotent() {
        let f1 = parse_sandbox_value(Some("allow-scripts allow-scripts allow-scripts"));
        let f2 = parse_sandbox_value(Some("allow-scripts"));
        assert_eq!(f1, f2);
    }
}
