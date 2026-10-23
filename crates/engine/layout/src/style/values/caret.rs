//! CSS UI L4 §6.2 — `caret-shape` и `caret-animation`; геометрия каретки по
//! форме. Оба свойства наследуются; `caret` — shorthand
//! `<'caret-color'> || <'caret-animation'> || <'caret-shape'>` (см.
//! `style::shorthand::apply_caret_shorthand`).

use crate::style::values::color::Color;
use lumen_core::geom::Rect;

/// `caret-shape: auto | bar | block | underscore`. Initial: `auto` (UA выбирает
/// форму; Lumen — `bar`, как у текстовых полей на всех платформах).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaretShape {
    #[default]
    Auto,
    Bar,
    Block,
    Underscore,
}

impl CaretShape {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "bar" => Some(Self::Bar),
            "block" => Some(Self::Block),
            "underscore" => Some(Self::Underscore),
            _ => None,
        }
    }

    pub fn to_css(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bar => "bar",
            Self::Block => "block",
            Self::Underscore => "underscore",
        }
    }

    /// Прямоугольник каретки на строке с верхом `top` и высотой `height`,
    /// начинающейся в `x`. `cell_w` — advance символа под кареткой, `bar` —
    /// толщина (≥ 1 физ. пикселя). `bar`/`auto` — вертикальная черта,
    /// `block` — вся ячейка символа, `underscore` — полоса у нижнего края ячейки.
    pub fn rect(self, x: f32, top: f32, height: f32, cell_w: f32, bar: f32) -> Rect {
        match self {
            Self::Auto | Self::Bar => Rect::new(x, top, bar, height),
            Self::Block => Rect::new(x, top, cell_w.max(bar), height),
            Self::Underscore => Rect::new(x, top + (height - bar).max(0.0), cell_w.max(bar), bar.min(height)),
        }
    }

    /// Цвет заливки. Блок закрывает глиф под собой, а перерисовки глифа в
    /// контрастном цвете в списке отображения нет, поэтому блок полупрозрачен:
    /// символ остаётся читаемым. Пробел: контрастный цвет текста под блоком (CSS UI L4 §6.2).
    pub fn paint_color(self, c: Color) -> Color {
        if self == Self::Block {
            Color { a: (u16::from(c.a) * 2 / 5) as u8, ..c }
        } else {
            c
        }
    }
}

/// `caret-animation: auto | manual`. Initial: `auto` (UA мигает кареткой);
/// `manual` — каретка статична, анимацию ведёт автор (`@keyframes` на
/// `caret-color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaretAnimation {
    #[default]
    Auto,
    Manual,
}

impl CaretAnimation {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }

    pub fn to_css(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_parse_accepts_only_the_four_keywords() {
        assert_eq!(CaretShape::parse("Block"), Some(CaretShape::Block));
        assert_eq!(CaretShape::parse(" underscore "), Some(CaretShape::Underscore));
        for bad in ["none", "ba", "underline", "vertical", "rect", ""] {
            assert_eq!(CaretShape::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn animation_parse_accepts_only_auto_and_manual() {
        assert_eq!(CaretAnimation::parse("manual"), Some(CaretAnimation::Manual));
        for bad in ["none", "manua", "fade", "blink"] {
            assert_eq!(CaretAnimation::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn rect_by_shape() {
        let bar = CaretShape::Bar.rect(10.0, 2.0, 20.0, 8.0, 1.0);
        assert_eq!((bar.x, bar.y, bar.width, bar.height), (10.0, 2.0, 1.0, 20.0));
        let block = CaretShape::Block.rect(10.0, 2.0, 20.0, 8.0, 1.0);
        assert_eq!((block.width, block.height), (8.0, 20.0));
        let us = CaretShape::Underscore.rect(10.0, 2.0, 20.0, 8.0, 2.0);
        assert_eq!((us.x, us.y, us.width, us.height), (10.0, 20.0, 8.0, 2.0));
    }
}
