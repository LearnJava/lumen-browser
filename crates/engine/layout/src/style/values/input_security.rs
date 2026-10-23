//! CSS UI L4 §6.5 — `input-security: auto | none`. Наследуется; анимируется дискретно.
//!
//! `auto` — чувствительное текстовое поле (`<input type=password>`) скрывает
//! вводимые символы; `none` — показывает текст как есть. На остальные элементы
//! свойство не влияет.

/// `input-security: auto | none`. Initial: `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputSecurity {
    #[default]
    Auto,
    None,
}

impl InputSecurity {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    pub fn to_css(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_only_auto_and_none() {
        assert_eq!(InputSecurity::parse(" NONE "), Some(InputSecurity::None));
        assert_eq!(InputSecurity::parse("auto"), Some(InputSecurity::Auto));
        for bad in ["auto auto", "disc", "circle", "square", "", "inert"] {
            assert_eq!(InputSecurity::parse(bad), None, "{bad}");
        }
    }
}
