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

pub mod avar;
pub mod binary;
pub mod cmap;
pub mod delta_set_index_map;
pub mod face;
pub mod fvar;
pub mod glyf;
pub mod gvar;
pub mod head;
pub mod hvar;
pub mod item_variation;
pub mod hhea;
pub mod hmtx;
pub mod loca;
pub mod maxp;
pub mod mvar;
pub mod name;
pub mod os2;
pub mod rasterizer;
pub mod system_fonts;
pub mod vvar;

pub use avar::{Avar, AxisValueMap, SegmentMap};
pub use binary::BinaryReader;
pub use cmap::Cmap;
pub use delta_set_index_map::{DeltaSetIndex, DeltaSetIndexMap};
pub use face::{Font, FontError, OffsetTable, TableRecord};
pub use fvar::{Fvar, NamedInstance, VariationAxis};
pub use glyf::{
    Anchor, BoundingBox, CompositeComponent, Contour, Glyf, Glyph, Outline, OutlinePoint,
};
pub use gvar::{GlyphVariationData, Gvar, PointNumbers, TupleVariation};
pub use head::{Head, IndexToLocFormat};
pub use hvar::Hvar;
pub use item_variation::{
    ItemVariationData, ItemVariationStore, RegionAxisCoordinates, VariationRegion,
    VariationRegionList,
};
pub use hhea::Hhea;
pub use hmtx::Hmtx;
pub use loca::Loca;
pub use maxp::Maxp;
pub use mvar::{Mvar, ValueRecord};
pub use name::Name;
pub use os2::Os2;
pub use rasterizer::{Bitmap, Rasterizer};
pub use system_fonts::SystemFontIndex;
pub use vvar::Vvar;
