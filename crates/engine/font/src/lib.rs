//! TrueType / OpenType парсер и растеризатор глифов для Lumen.
//!
//! Phase 0 минимум: загрузить шрифт, найти таблицы по тегу, прочитать
//! cmap → glyph index, glyf → outline, обнаружить advance widths из hmtx,
//! растеризовать outline в bitmap. Этого достаточно для текста Latin/
//! Cyrillic без hinting, kerning и ligatures.
//!
//! TTF/OTF — формат с большим количеством обязательных и опциональных
//! таблиц; реализуем по мере необходимости. Не поддерживается (отложено):
//! composite glyphs, hinting (TT instructions), GPOS/GSUB (advanced shaping),
//! CFF outlines (для PostScript-OpenType), variable fonts, color glyphs
//! (COLR/CPAL, sbix), bitmap strikes (EBDT/EBLC).

pub mod binary;
pub mod face;

pub use binary::BinaryReader;
pub use face::{Font, FontError, OffsetTable, TableRecord};
