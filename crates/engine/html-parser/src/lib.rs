//! HTML-парсер для Lumen.
//!
//! Phase 0 — минимальный токенизатор (`tokenizer`) + lenient tree builder
//! (`tree_builder`). Удобный вход — функция [`parse`]: строка → [`lumen_dom::Document`].
//!
//! Что поддерживается: открывающие/закрывающие/самозакрывающиеся теги,
//! атрибуты (quoted/unquoted), комментарии, базовые character references,
//! void-элементы, lenient end-tag matching.
//!
//! Что не поддерживается (отложено до Phase 1+): CDATA, insertion modes
//! (in_table, in_select), полный набор named entities (~2125 имён —
//! у нас 250+ самых частых), foster parent reparenting.

mod entities;
pub mod picture;
pub mod preload_scanner;
pub mod quirks_mode;
pub mod srcset;
pub mod tokenizer;
pub mod tree_builder;

pub use picture::{PickedSource, PictureParams, pick_img_source, pick_picture_source};
pub use preload_scanner::{PreloadHint, scan_preload_hints};
pub use quirks_mode::detect_document_mode;
pub use srcset::{
    ColorScheme, MediaClause, MediaCondition, Orientation, SizeLength, SizesViewport, SourceSize,
    SrcsetCandidate, SrcsetDescriptor, evaluate_sizes, parse_media_condition, parse_sizes,
    parse_srcset, pick_best_for_density, pick_best_for_width,
};
pub use tokenizer::{Token, Tokenizer};
pub use tree_builder::parse;
