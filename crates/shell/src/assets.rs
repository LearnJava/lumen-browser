//! Assets the shell holds for the whole process: the Inter face compiled into
//! the binary, and the Hunspell dictionaries read from the portable
//! `data/spell/` folder the first time a window needs them.
//!
//! Both are process-global on purpose. The font bytes are the fallback every
//! path that needs a font backend starts from — window, headless dump, SVG
//! rasterization — and the dictionaries cost a filesystem walk that must not
//! be repeated per tab.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`; only visibility
//! changed.

use crate::*;

/// Bundled-шрифт: статический Inter v4.1 Regular (~411 КБ),
/// SIL OFL 1.1, см. assets/fonts/OFL.txt.
pub(crate) const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// P3-spell срез 2: словари Hunspell, загруженные фоновым потоком при старте
/// окна из `data/spell/` (`spellcheck::load_dictionaries`). До завершения
/// загрузки `get()` возвращает `None` и спелл-чек молчит.
pub(crate) static SPELL_DICTS: std::sync::OnceLock<spellcheck::MultiDictionary> = std::sync::OnceLock::new();
