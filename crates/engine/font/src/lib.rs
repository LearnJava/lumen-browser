//! TrueType / OpenType парсер и растеризатор глифов для Lumen.
//!
//! Phase 0 минимум: загрузить шрифт, найти таблицы по тегу, прочитать
//! cmap → glyph index, glyf → outline, обнаружить advance widths из hmtx,
//! растеризовать outline в bitmap. Этого достаточно для текста Latin/
//! Cyrillic без hinting, kerning и ligatures.
//!
//! TTF/OTF — формат с большим количеством обязательных и опциональных
//! таблиц; реализуем по мере необходимости. Не поддерживается (отложено):
//! hinting (TT instructions), GPOS/GSUB (advanced shaping), CFF outlines
//! (для PostScript-OpenType), variable fonts, color glyphs (COLR/CPAL,
//! sbix), bitmap strikes (EBDT/EBLC).

pub mod binary;
pub mod cmap;
pub mod face;
pub mod glyf;
pub mod head;
pub mod hhea;
pub mod hmtx;
pub mod loca;
pub mod maxp;
pub mod name;
pub mod os2;
pub mod rasterizer;
pub mod system_fonts;

pub use binary::BinaryReader;
pub use cmap::Cmap;
pub use face::{Font, FontError, OffsetTable, TableRecord};
pub use glyf::{BoundingBox, CompositeComponent, Contour, Glyf, Glyph, Outline, OutlinePoint};
pub use head::{Head, IndexToLocFormat};
pub use hhea::Hhea;
pub use hmtx::Hmtx;
pub use loca::Loca;
pub use maxp::Maxp;
pub use name::Name;
pub use os2::Os2;
pub use rasterizer::{Bitmap, Rasterizer};
pub use system_fonts::SystemFontIndex;
