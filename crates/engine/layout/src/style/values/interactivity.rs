//! CSS UI L4 §6.4 — `interactivity: auto | inert`. Наследуется; не анимируется.
//!
//! `inert` делает элемент и его потомков по плоскому дереву неинтерактивными:
//! hit-test не отдаёт их как цель указателя. HTML Rendering сопоставляет
//! `[inert]`, модальному `<dialog>` и `:fullscreen` значения этого свойства
//! (см. `style::ua::apply_ua_interactivity_early` / `apply_ua_inert_important`).

/// `interactivity: auto | inert`. Initial: `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interactivity {
    #[default]
    Auto,
    Inert,
}

impl Interactivity {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "inert" => Some(Self::Inert),
            _ => None,
        }
    }

    pub fn to_css(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Inert => "inert",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_only_auto_and_inert() {
        assert_eq!(Interactivity::parse(" INERT "), Some(Interactivity::Inert));
        assert_eq!(Interactivity::parse("auto"), Some(Interactivity::Auto));
        for bad in ["none", "normal", "auto inert", "", "inert!"] {
            assert_eq!(Interactivity::parse(bad), None, "{bad}");
        }
    }
}
