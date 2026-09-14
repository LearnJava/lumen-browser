//! HTML-парсер для Lumen.
//!
//! Phase 0 — минимальный токенизатор (`tokenizer`) + lenient tree builder
//! (`tree_builder`). Удобный вход — функция [`parse`]: строка → [`lumen_dom::Document`].
//!
//! Что поддерживается: открывающие/закрывающие/самозакрывающиеся теги,
//! атрибуты (quoted/unquoted), комментарии, базовые character references,
//! void-элементы, lenient end-tag matching.
//!
//! Что не поддерживается (отложено до Phase 1+): полный набор named
//! entities (~2125 имён — у нас 250+ самых частых). `<![CDATA[...]]>` —
//! только в pull-режиме (`parse`/[`parse_fragment`]/
//! [`parse_fragment_with_context`]), namespace-зависимо (GAP-XMLDOC срез
//! 14, BUG-685); `PushTokenizer` (сетевая загрузка) её не различает.

mod entities;
mod foreign_content;
pub mod picture;
pub mod preload_scanner;
pub mod push_tokenizer;
pub mod quirks_mode;
pub mod srcset;
pub mod tokenizer;
pub mod tree_builder;
mod xml_cdata;

pub use picture::{PickedSource, PictureParams, pick_img_source, pick_picture_source};
pub use preload_scanner::{PreloadHint, PreloadScanner, scan_preload_hints};
pub use push_tokenizer::PushTokenizer;
pub use quirks_mode::detect_document_mode;
pub use srcset::{
    ColorScheme, MediaClause, MediaCondition, Orientation, SizeLength, SizesViewport, SourceSize,
    SrcsetCandidate, SrcsetDescriptor, evaluate_sizes, parse_media_condition, parse_sizes,
    parse_srcset, pick_best_for_density, pick_best_for_width,
};
pub use tokenizer::{Token, Tokenizer};
pub use tree_builder::{
    FragmentContext, IncrementalTreeBuilder, parse, parse_fragment, parse_fragment_with_context,
    parse_xml_flavoured,
};
