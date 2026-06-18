//! TrueType / OpenType парсер и растеризатор глифов для Lumen.
//!
//! Phase 0 минимум: загрузить шрифт, найти таблицы по тегу, прочитать
//! cmap → glyph index, glyf → outline, обнаружить advance widths из hmtx,
//! растеризовать outline в bitmap. Этого достаточно для текста Latin/
//! Cyrillic без hinting, kerning и ligatures.
//!
//! TTF/OTF — формат с большим количеством обязательных и опциональных
//! таблиц; реализуем по мере необходимости. Шейпинг (U-2 этап 1): GSUB
//! лигатуры (`liga`/`clig`) + GPOS кернинг (`kern`) для Latin/Cyrillic —
//! см. [`shape::Shaper`]. Не поддерживается (отложено): hinting (TT
//! instructions), CFF outlines (для PostScript-OpenType, U-2 этап 2),
//! сложные скрипты / mark-позиционирование, color glyphs (COLR/CPAL,
//! sbix), bitmap strikes (EBDT/EBLC).

pub mod avar;
pub mod binary;
pub mod unicode_range;
pub mod woff2;
pub mod cmap;
pub mod delta_set_index_map;
pub mod face;
pub mod fvar;
pub mod glyf;
pub mod gpos;
pub mod gsub;
pub mod gvar;
pub mod head;
pub mod otlayout;
pub mod shape;
pub mod hvar;
pub mod item_variation;
pub mod hhea;
pub mod hmtx;
pub mod loca;
pub mod maxp;
pub mod mvar;
pub mod name;
pub mod os2;
pub mod post;
pub mod rasterizer;
pub mod font_registry;
pub mod system_fonts;
pub mod variation;
pub mod variation_coords;
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
pub use gpos::Gpos;
pub use gsub::Gsub;
pub use gvar::{GlyphVariationData, Gvar, PointNumbers, TupleVariation};
pub use head::{Head, IndexToLocFormat};
pub use shape::{ShapedGlyph, Shaper};
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
pub use post::Post;
pub use rasterizer::{Bitmap, Rasterizer};
pub use font_registry::FontRegistry;
pub use unicode_range::{UnicodeRange, parse_unicode_ranges, codepoint_in_ranges};
pub use system_fonts::SystemFontIndex;
pub use variation::apply_variations_to_simple_outline;
pub use variation_coords::VariationCoords;
pub use vvar::Vvar;
pub use woff2::{decode_woff1, decode_woff2, is_woff1, is_woff2, maybe_decode_font};
