//! Predefined counter styles — CSS Counter Styles L3 §6 (BUG-1570).
//!
//! `counter(n, hebrew)` and `list-style-type: cjk-decimal` resolve here when no
//! author `@counter-style` of that name shadows them. The table-driven styles
//! (numeric digits, additive Hebrew/Armenian/Georgian, alphabetic kana, fixed
//! CJK cycles, disclosure signs) are plain [`CounterStyleDef`]s built once;
//! the two algorithmic families — the longhand East Asian styles (§6.5) and
//! `ethiopic-numeric` (§6.4) — are [`BuiltinSystem`]s that produce their
//! representation in code.
//!
//! Lookup is ASCII case-insensitive: predefined names are CSS keywords.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::counters::{CounterRange, CounterStyleDef, CounterSystem, RangeBound};

/// Algorithmic counter systems that cannot be written as `symbols`/`additive-symbols`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinSystem {
    /// `japanese-*`, `korean-*`, `simp-chinese-*`, `trad-chinese-*` — §6.5.
    EastAsian(&'static EastAsian),
    /// `ethiopic-numeric` — §6.4.
    Ethiopic,
}

impl BuiltinSystem {
    /// Representation of the non-negative `n` (the sign is added by the
    /// `negative` descriptor), or `None` when the algorithm cannot express it.
    pub fn represent(self, n: i64) -> Option<String> {
        match self {
            Self::EastAsian(spec) => spec.represent(n),
            Self::Ethiopic => ethiopic(n),
        }
    }
}

/// How the digit `1` is dropped in front of a place marker (`十`, `百`, `千`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropOne {
    /// Formal styles keep every digit.
    Never,
    /// Japanese / Korean informal: `十`, `百`, `千` stand alone.
    BeforePlace,
    /// Chinese informal: only a lone `十` (`10`‥`19` inside a group) drops `一`.
    ChineseTen,
}

/// Parameters of one longhand East Asian counter style.
#[derive(Debug, PartialEq, Eq)]
pub struct EastAsian {
    digits: [&'static str; 10],
    /// Place markers for 10, 100, 1000.
    places: [&'static str; 3],
    /// Group markers for 10^4, 10^8, 10^12.
    groups: [&'static str; 3],
    drop_one: DropOne,
    /// Korean informal: `一萬` is written `萬` (but `一億`, `一兆` keep the digit).
    drop_one_before_man: bool,
    /// Chinese: `零` written where a run of zeros interrupts the number.
    gap_zero: bool,
    /// Korean: groups are separated by a space.
    group_sep: &'static str,
}

/// Counter values stay inside ±(10^16 − 1) — beyond it the style falls back.
const EAST_ASIAN_LIMIT: i64 = 10_000_000_000_000_000;

impl EastAsian {
    fn represent(&self, n: i64) -> Option<String> {
        if n >= EAST_ASIAN_LIMIT {
            return None;
        }
        if n == 0 {
            return Some(self.digits[0].to_string());
        }
        let mut groups = [0u32; 4];
        let mut rest = n;
        for g in &mut groups {
            *g = (rest % 10_000) as u32;
            rest /= 10_000;
        }
        let mut parts: Vec<String> = Vec::new();
        // Group index of the previous (higher) non-zero group, to know if an all-zero group was skipped.
        let mut prev: Option<usize> = None;
        for i in (0..4).rev() {
            let g = groups[i];
            if g == 0 {
                continue;
            }
            let mut piece = String::new();
            if self.gap_zero
                && let Some(p) = prev
                && (p > i + 1 || g < 1000)
            {
                piece.push_str(self.digits[0]);
            }
            let lone_one_man = self.drop_one_before_man && i == 1 && g == 1;
            if !lone_one_man {
                piece.push_str(&self.group_body(g));
            }
            if i > 0 {
                piece.push_str(self.groups[i - 1]);
            }
            parts.push(piece);
            prev = Some(i);
        }
        Some(parts.join(self.group_sep))
    }

    /// One 4-digit group (1‥9999) without its group marker.
    fn group_body(&self, g: u32) -> String {
        let digit = |d: u32| self.digits[d as usize];
        let (th, h, t, o) = (g / 1000, g / 100 % 10, g / 10 % 10, g % 10);
        let mut out = String::new();
        // A zero digit between two non-zero ones is written once (Chinese only).
        let mut pending_zero = false;
        for (d, marker) in [(th, self.places[2]), (h, self.places[1]), (t, self.places[0])] {
            if d == 0 {
                pending_zero = !out.is_empty();
                continue;
            }
            if pending_zero && self.gap_zero {
                out.push_str(self.digits[0]);
            }
            pending_zero = false;
            let drop = d == 1
                && match self.drop_one {
                    DropOne::Never => false,
                    DropOne::BeforePlace => true,
                    DropOne::ChineseTen => marker == self.places[0] && th == 0 && h == 0,
                };
            if !drop {
                out.push_str(digit(d));
            }
            out.push_str(marker);
        }
        if o != 0 {
            if pending_zero && self.gap_zero {
                out.push_str(self.digits[0]);
            }
            out.push_str(digit(o));
        }
        out
    }
}

static SIMP_CHINESE_INFORMAL: EastAsian = EastAsian {
    digits: ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"],
    places: ["十", "百", "千"],
    groups: ["万", "亿", "万亿"],
    drop_one: DropOne::ChineseTen,
    drop_one_before_man: false,
    gap_zero: true,
    group_sep: "",
};
static SIMP_CHINESE_FORMAL: EastAsian = EastAsian {
    digits: ["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"],
    places: ["拾", "佰", "仟"],
    groups: ["万", "亿", "万亿"],
    drop_one: DropOne::Never,
    drop_one_before_man: false,
    gap_zero: true,
    group_sep: "",
};
static TRAD_CHINESE_INFORMAL: EastAsian = EastAsian {
    digits: ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"],
    places: ["十", "百", "千"],
    groups: ["萬", "億", "兆"],
    drop_one: DropOne::ChineseTen,
    drop_one_before_man: false,
    gap_zero: true,
    group_sep: "",
};
static TRAD_CHINESE_FORMAL: EastAsian = EastAsian {
    digits: ["零", "壹", "貳", "參", "肆", "伍", "陸", "柒", "捌", "玖"],
    places: ["拾", "佰", "仟"],
    groups: ["萬", "億", "兆"],
    drop_one: DropOne::Never,
    drop_one_before_man: false,
    gap_zero: true,
    group_sep: "",
};
static JAPANESE_INFORMAL: EastAsian = EastAsian {
    digits: ["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"],
    places: ["十", "百", "千"],
    groups: ["万", "億", "兆"],
    drop_one: DropOne::BeforePlace,
    drop_one_before_man: false,
    gap_zero: false,
    group_sep: "",
};
static JAPANESE_FORMAL: EastAsian = EastAsian {
    digits: ["零", "壱", "弐", "参", "四", "伍", "六", "七", "八", "九"],
    places: ["拾", "百", "阡"],
    groups: ["萬", "億", "兆"],
    drop_one: DropOne::Never,
    drop_one_before_man: false,
    gap_zero: false,
    group_sep: "",
};
static KOREAN_HANGUL_FORMAL: EastAsian = EastAsian {
    digits: ["영", "일", "이", "삼", "사", "오", "육", "칠", "팔", "구"],
    places: ["십", "백", "천"],
    groups: ["만", "억", "조"],
    drop_one: DropOne::Never,
    drop_one_before_man: false,
    gap_zero: false,
    group_sep: " ",
};
static KOREAN_HANJA_FORMAL: EastAsian = EastAsian {
    digits: ["零", "壹", "貳", "參", "四", "五", "六", "七", "八", "九"],
    places: ["拾", "百", "仟"],
    groups: ["萬", "億", "兆"],
    drop_one: DropOne::Never,
    drop_one_before_man: false,
    gap_zero: false,
    group_sep: " ",
};
static KOREAN_HANJA_INFORMAL: EastAsian = EastAsian {
    digits: ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"],
    places: ["十", "百", "千"],
    groups: ["萬", "億", "兆"],
    drop_one: DropOne::BeforePlace,
    drop_one_before_man: true,
    gap_zero: false,
    group_sep: " ",
};

/// `ethiopic-numeric` (§6.4): pairs of digits with `፻` (100) and `፼` (10 000).
fn ethiopic(n: i64) -> Option<String> {
    const ONES: [char; 9] = ['፩', '፪', '፫', '፬', '፭', '፮', '፯', '፰', '፱'];
    const TENS: [char; 9] = ['፲', '፳', '፴', '፵', '፶', '፷', '፸', '፹', '፺'];
    fn pair(v: i64, out: &mut String) {
        if v >= 10 {
            out.push(TENS[(v / 10 - 1) as usize]);
        }
        if v % 10 != 0 {
            out.push(ONES[(v % 10 - 1) as usize]);
        }
    }
    // A number below 10 000: hundreds-pair `፻` units-pair, a lone `፩` before `፻` omitted.
    fn below_myriad(v: i64, out: &mut String) {
        let (hi, lo) = (v / 100, v % 100);
        if hi > 0 {
            if hi > 1 {
                pair(hi, out);
            }
            out.push('፻');
        }
        pair(lo, out);
    }
    fn go(n: i64, out: &mut String) {
        if n >= 10_000 {
            let q = n / 10_000;
            if q > 1 {
                go(q, out);
            }
            out.push('፼');
        }
        below_myriad(n % 10_000, out);
    }
    if n < 1 {
        return None;
    }
    let mut out = String::new();
    go(n, &mut out);
    Some(out)
}

fn explicit_range(min: i32, max: i32) -> CounterRange {
    CounterRange::Explicit(vec![RangeBound { min: Some(min), max: Some(max) }])
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// Ten consecutive code points starting at `zero`, as `symbols` of a `numeric` style.
fn digits_from(zero: u32) -> Vec<String> {
    (0..10).filter_map(|i| char::from_u32(zero + i)).map(String::from).collect()
}

fn numeric(zero: u32) -> CounterStyleDef {
    CounterStyleDef {
        system: CounterSystem::Numeric,
        symbols: digits_from(zero),
        suffix: ". ".to_string(),
        ..CounterStyleDef::default()
    }
}

/// Additive style from a `(weight, glyphs)` table written in descending order.
fn additive(table: &[(i32, &str)], max: i32) -> CounterStyleDef {
    CounterStyleDef {
        system: CounterSystem::Additive,
        additive_symbols: table.iter().map(|&(w, s)| (w, s.to_string())).collect(),
        range: explicit_range(1, max),
        suffix: ". ".to_string(),
        ..CounterStyleDef::default()
    }
}

/// Armenian numerals: `base` is the code point of the symbol for 1 (`Ա` / `ա`).
fn armenian(base: u32) -> CounterStyleDef {
    let glyph = |i: u32| char::from_u32(base + i).map(String::from).unwrap_or_default();
    // 1‥9, 10‥90, 100‥900, 1000‥9000 occupy 36 consecutive code points.
    let mut table = Vec::with_capacity(36);
    for i in (0..36u32).rev() {
        let unit = 10i32.pow(i / 9);
        table.push((unit * (i % 9 + 1) as i32, glyph(i)));
    }
    CounterStyleDef {
        system: CounterSystem::Additive,
        additive_symbols: table,
        range: explicit_range(1, 9999),
        suffix: ". ".to_string(),
        ..CounterStyleDef::default()
    }
}

fn alphabetic(symbols: &str) -> CounterStyleDef {
    CounterStyleDef {
        system: CounterSystem::Alphabetic,
        symbols: symbols.chars().map(String::from).collect(),
        suffix: "、".to_string(),
        ..CounterStyleDef::default()
    }
}

fn fixed_cjk(symbols: &str) -> CounterStyleDef {
    CounterStyleDef {
        system: CounterSystem::Fixed(1),
        symbols: symbols.chars().map(String::from).collect(),
        suffix: "、".to_string(),
        fallback: "cjk-decimal".to_string(),
        ..CounterStyleDef::default()
    }
}

fn east_asian(spec: &'static EastAsian, negative: &str, suffix: &str) -> CounterStyleDef {
    let limit = (EAST_ASIAN_LIMIT - 1).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    CounterStyleDef {
        system: CounterSystem::Builtin(BuiltinSystem::EastAsian(spec)),
        range: explicit_range(-limit, limit),
        negative: (negative.to_string(), String::new()),
        suffix: suffix.to_string(),
        fallback: "cjk-decimal".to_string(),
        ..CounterStyleDef::default()
    }
}

fn disclosure(sign: &str) -> CounterStyleDef {
    CounterStyleDef {
        system: CounterSystem::Cyclic,
        symbols: strings(&[sign]),
        suffix: " ".to_string(),
        ..CounterStyleDef::default()
    }
}

fn build() -> HashMap<&'static str, CounterStyleDef> {
    let mut m: HashMap<&'static str, CounterStyleDef> = HashMap::new();

    // §6.1 Numeric systems.
    for (name, zero) in [
        ("arabic-indic", 0x0660),
        ("persian", 0x06F0),
        ("bengali", 0x09E6),
        ("devanagari", 0x0966),
        ("gujarati", 0x0AE6),
        ("gurmukhi", 0x0A66),
        ("kannada", 0x0CE6),
        ("khmer", 0x17E0),
        ("cambodian", 0x17E0),
        ("lao", 0x0ED0),
        ("malayalam", 0x0D66),
        ("mongolian", 0x1810),
        ("myanmar", 0x1040),
        ("oriya", 0x0B66),
        ("tamil", 0x0BE6),
        ("telugu", 0x0C66),
        ("thai", 0x0E50),
        ("tibetan", 0x0F20),
    ] {
        m.insert(name, numeric(zero));
    }
    m.insert(
        "cjk-decimal",
        CounterStyleDef {
            system: CounterSystem::Numeric,
            symbols: strings(&["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"]),
            range: CounterRange::Explicit(vec![RangeBound { min: Some(0), max: None }]),
            suffix: "、".to_string(),
            ..CounterStyleDef::default()
        },
    );

    // §6.2 Alphabetic and fixed systems.
    m.insert(
        "hiragana",
        alphabetic("あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわゐゑをん"),
    );
    m.insert(
        "hiragana-iroha",
        alphabetic("いろはにほへとちりぬるをわかよたれそつねならむうゐのおくやまけふこえてあさきゆめみしゑひもせす"),
    );
    m.insert(
        "katakana",
        alphabetic("アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヰヱヲン"),
    );
    m.insert(
        "katakana-iroha",
        alphabetic("イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス"),
    );
    m.insert("cjk-earthly-branch", fixed_cjk("子丑寅卯辰巳午未申酉戌亥"));
    m.insert("cjk-heavenly-stem", fixed_cjk("甲乙丙丁戊己庚辛壬癸"));

    // §6.3 Additive systems.
    m.insert("armenian", armenian(0x0531));
    m.insert("upper-armenian", armenian(0x0531));
    m.insert("lower-armenian", armenian(0x0561));
    m.insert(
        "georgian",
        additive(
            &[
                (10000, "ჵ"), (9000, "ჰ"), (8000, "ჯ"), (7000, "ჴ"), (6000, "ხ"), (5000, "ჭ"),
                (4000, "წ"), (3000, "ძ"), (2000, "ც"), (1000, "ჩ"), (900, "შ"), (800, "ყ"),
                (700, "ღ"), (600, "ქ"), (500, "ფ"), (400, "ჳ"), (300, "ტ"), (200, "ს"),
                (100, "რ"), (90, "ჟ"), (80, "პ"), (70, "ო"), (60, "ჲ"), (50, "ნ"), (40, "მ"),
                (30, "ლ"), (20, "კ"), (10, "ი"), (9, "თ"), (8, "ჱ"), (7, "ზ"), (6, "ვ"),
                (5, "ე"), (4, "დ"), (3, "გ"), (2, "ბ"), (1, "ა"),
            ],
            19999,
        ),
    );
    m.insert(
        "hebrew",
        additive(
            &[
                (10000, "י׳"), (9000, "ט׳"), (8000, "ח׳"), (7000, "ז׳"), (6000, "ו׳"),
                (5000, "ה׳"), (4000, "ד׳"), (3000, "ג׳"), (2000, "ב׳"), (1000, "א׳"),
                (400, "ת"), (300, "ש"), (200, "ר"), (100, "ק"), (90, "צ"), (80, "פ"),
                (70, "ע"), (60, "ס"), (50, "נ"), (40, "מ"), (30, "ל"), (20, "כ"),
                (19, "יט"), (18, "יח"), (17, "יז"), (16, "טז"), (15, "טו"), (10, "י"),
                (9, "ט"), (8, "ח"), (7, "ז"), (6, "ו"), (5, "ה"), (4, "ד"), (3, "ג"),
                (2, "ב"), (1, "א"),
            ],
            10999,
        ),
    );

    // §6.4 Ethiopic.
    m.insert(
        "ethiopic-numeric",
        CounterStyleDef {
            system: CounterSystem::Builtin(BuiltinSystem::Ethiopic),
            range: CounterRange::Explicit(vec![RangeBound { min: Some(1), max: None }]),
            suffix: "/ ".to_string(),
            ..CounterStyleDef::default()
        },
    );

    // §6.5 Longhand East Asian counter styles.
    m.insert("simp-chinese-informal", east_asian(&SIMP_CHINESE_INFORMAL, "负", "、"));
    m.insert("simp-chinese-formal", east_asian(&SIMP_CHINESE_FORMAL, "负", "、"));
    m.insert("trad-chinese-informal", east_asian(&TRAD_CHINESE_INFORMAL, "負", "、"));
    m.insert("trad-chinese-formal", east_asian(&TRAD_CHINESE_FORMAL, "負", "、"));
    m.insert("japanese-informal", east_asian(&JAPANESE_INFORMAL, "マイナス", "、"));
    m.insert("japanese-formal", east_asian(&JAPANESE_FORMAL, "マイナス", "、"));
    m.insert("korean-hangul-formal", east_asian(&KOREAN_HANGUL_FORMAL, "마이너스 ", ", "));
    m.insert("korean-hanja-formal", east_asian(&KOREAN_HANJA_FORMAL, "마이너스 ", ", "));
    m.insert("korean-hanja-informal", east_asian(&KOREAN_HANJA_INFORMAL, "마이너스 ", ", "));

    // §6.6 Symbolic: disclosure widgets (left-to-right glyphs).
    m.insert("disclosure-open", disclosure("\u{25BE}"));
    m.insert("disclosure-closed", disclosure("\u{25B8}"));

    m
}

/// The predefined counter style named `name`, if there is one.
pub fn lookup(name: &str) -> Option<&'static CounterStyleDef> {
    static STYLES: OnceLock<HashMap<&'static str, CounterStyleDef>> = OnceLock::new();
    let styles = STYLES.get_or_init(build);
    let name = name.trim();
    styles.get(name).or_else(|| {
        // Predefined names are keywords: `HEBREW` is `hebrew`.
        if name.bytes().any(|b| b.is_ascii_uppercase()) {
            styles.get(name.to_ascii_lowercase().as_str())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::{format_counter_with_registry, CounterStyleRegistry};

    fn rep(name: &str, n: i32) -> String {
        format_counter_with_registry(n, name, &CounterStyleRegistry::new())
    }

    #[test]
    fn hebrew_matches_spec_examples() {
        assert_eq!(rep("hebrew", 1), "א");
        assert_eq!(rep("hebrew", 9), "ט");
        assert_eq!(rep("hebrew", 15), "טו");
        assert_eq!(rep("hebrew", 16), "טז");
        assert_eq!(rep("hebrew", 115), "קטו");
        assert_eq!(rep("hebrew", 11000), "11000");
    }

    #[test]
    fn armenian_additive_table() {
        assert_eq!(rep("armenian", 1), "Ա");
        assert_eq!(rep("armenian", 11), "ԺԱ");
        assert_eq!(rep("upper-armenian", 9999), "ՔՋՂԹ");
        assert_eq!(rep("lower-armenian", 1000), "ռ");
        assert_eq!(rep("armenian", 10000), "10000");
    }

    #[test]
    fn georgian_range_is_nineteen_thousand() {
        assert_eq!(rep("georgian", 10), "ი");
        assert_eq!(rep("georgian", 10000), "ჵ");
        assert_eq!(rep("georgian", 19999), "ჵჰშჟთ");
        assert_eq!(rep("georgian", 20000), "20000");
    }

    #[test]
    fn numeric_scripts() {
        assert_eq!(rep("arabic-indic", 1024), "١٠٢٤");
        assert_eq!(rep("thai", 90), "๙๐");
        assert_eq!(rep("cjk-decimal", 0), "〇");
        assert_eq!(rep("cjk-decimal", 12345), "一二三四五");
        assert_eq!(rep("cjk-decimal", -1), "-1");
        assert_eq!(rep("cambodian", 5), rep("khmer", 5));
    }

    #[test]
    fn alphabetic_and_fixed_cjk() {
        assert_eq!(rep("hiragana", 1), "あ");
        assert_eq!(rep("hiragana", 48), "ん");
        assert_eq!(rep("hiragana", 49), "ああ");
        assert_eq!(rep("katakana-iroha", 2), "ロ");
        assert_eq!(rep("cjk-earthly-branch", 12), "亥");
        assert_eq!(rep("cjk-earthly-branch", 13), "一三");
        assert_eq!(rep("cjk-heavenly-stem", 11), "一一");
    }

    #[test]
    fn chinese_informal_zero_and_ten() {
        assert_eq!(rep("simp-chinese-informal", 0), "零");
        assert_eq!(rep("simp-chinese-informal", 10), "十");
        assert_eq!(rep("simp-chinese-informal", 11), "十一");
        assert_eq!(rep("simp-chinese-informal", 100), "一百");
        assert_eq!(rep("simp-chinese-informal", 101), "一百零一");
        assert_eq!(rep("simp-chinese-informal", 110), "一百一十");
        assert_eq!(rep("simp-chinese-informal", 6001), "六千零一");
        assert_eq!(rep("simp-chinese-informal", 10010), "一万零十");
        assert_eq!(rep("simp-chinese-informal", 11000), "一万一千");
        assert_eq!(rep("simp-chinese-informal", 100010000), "一亿零一万");
        assert_eq!(rep("simp-chinese-informal", 110000000), "一亿一千万");
        assert_eq!(rep("simp-chinese-informal", 1000000000), "十亿");
        assert_eq!(rep("trad-chinese-informal", -10000), "負一萬");
    }

    #[test]
    fn chinese_formal_keeps_every_digit() {
        assert_eq!(rep("simp-chinese-formal", 10), "壹拾");
        assert_eq!(rep("trad-chinese-formal", 12345), "壹萬貳仟參佰肆拾伍");
        assert_eq!(rep("trad-chinese-formal", 1000000000), "壹拾億");
    }

    #[test]
    fn japanese_has_no_gap_zero() {
        assert_eq!(rep("japanese-informal", 100), "百");
        assert_eq!(rep("japanese-informal", 10010), "一万十");
        assert_eq!(rep("japanese-informal", 6001), "六千一");
        assert_eq!(rep("japanese-formal", 6001), "六阡壱");
        assert_eq!(rep("japanese-formal", 12345), "壱萬弐阡参百四拾伍");
        assert_eq!(rep("japanese-formal", -1), "マイナス壱");
    }

    #[test]
    fn korean_groups_are_space_separated() {
        assert_eq!(rep("korean-hangul-formal", 12345), "일만 이천삼백사십오");
        assert_eq!(rep("korean-hanja-informal", 10000), "萬");
        assert_eq!(rep("korean-hanja-informal", 100000000), "一億");
        assert_eq!(rep("korean-hanja-informal", 11000), "萬 千");
        assert_eq!(rep("korean-hanja-formal", -10000), "마이너스 壹萬");
    }

    #[test]
    fn ethiopic_pairs() {
        assert_eq!(rep("ethiopic-numeric", 1), "፩");
        assert_eq!(rep("ethiopic-numeric", 100), "፻");
        assert_eq!(rep("ethiopic-numeric", 101), "፻፩");
        assert_eq!(rep("ethiopic-numeric", 1000), "፲፻");
        assert_eq!(rep("ethiopic-numeric", 7005), "፸፻፭");
        assert_eq!(rep("ethiopic-numeric", 10000), "፼");
        assert_eq!(rep("ethiopic-numeric", 1000001), "፻፼፩");
        assert_eq!(rep("ethiopic-numeric", 78010092), "፸፰፻፩፼፺፪");
        assert_eq!(rep("ethiopic-numeric", 0), "0");
        assert_eq!(rep("ethiopic-numeric", -1), "-1");
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(lookup("HEBREW").is_some());
        assert!(lookup("nonexistent").is_none());
    }
}

#[cfg(test)]
mod wpt_cases {
    use crate::counters::{format_counter_with_registry, CounterStyleRegistry};

    /// Every row of the table extracted from the WPT references must agree.
    #[test]
    fn representations_match_wpt_references() {
        let registry = CounterStyleRegistry::new();
        let mut bad = Vec::new();
        for &(style, value, expected) in crate::counter_builtin_cases::CASES {
            let got = format_counter_with_registry(value, style, &registry);
            if !expected.split('|').any(|alt| alt == got) {
                bad.push(format!("{style} {value}: got {got:?}, want {expected:?}"));
            }
        }
        assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
    }
}
