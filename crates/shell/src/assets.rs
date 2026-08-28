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

/// Bundled-С€СЂРёС„С‚: СЃС‚Р°С‚РёС‡РµСЃРєРёР№ Inter v4.1 Regular (~411 РљР‘),
/// SIL OFL 1.1, СЃРј. assets/fonts/OFL.txt.
pub(crate) const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// P3-spell СЃСЂРµР· 2: СЃР»РѕРІР°СЂРё Hunspell, Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ С„РѕРЅРѕРІС‹Рј РїРѕС‚РѕРєРѕРј РїСЂРё СЃС‚Р°СЂС‚Рµ
/// РѕРєРЅР° РёР· `data/spell/` (`spellcheck::load_dictionaries`). Р”Рѕ Р·Р°РІРµСЂС€РµРЅРёСЏ
/// Р·Р°РіСЂСѓР·РєРё `get()` РІРѕР·РІСЂР°С‰Р°РµС‚ `None` Рё СЃРїРµР»Р»-С‡РµРє РјРѕР»С‡РёС‚.
pub(crate) static SPELL_DICTS: std::sync::OnceLock<spellcheck::MultiDictionary> = std::sync::OnceLock::new();
