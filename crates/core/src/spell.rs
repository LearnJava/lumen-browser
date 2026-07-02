//! Pure-Rust spell checking engine over the Hunspell dictionary format
//! (.aff / .dic subset). Implements [`crate::ext::SpellChecker`].

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::ext::SpellChecker;

/// Ошибка загрузки Hunspell-словаря.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellError {
    /// В .dic не удалось разобрать ни одного слова.
    EmptyDictionary,
}

impl fmt::Display for SpellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpellError::EmptyDictionary => write!(f, "dictionary contains no words"),
        }
    }
}

impl Error for SpellError {}

/// Hunspell-словарь (.aff/.dic), развёрнутый в память при загрузке.
#[derive(Debug)]
pub struct HunspellDictionary {
    words: HashSet<String>,
    words_lower: HashSet<String>,
    try_chars: Vec<char>,
    locale: String,
}

impl HunspellDictionary {
    /// Разбирает тексты .aff и .dic, разворачивает аффиксные формы в набор слов.
    ///
    /// # Errors
    /// `SpellError::EmptyDictionary` — если из .dic не извлечено ни одного слова.
    pub fn from_aff_dic(aff: &str, dic: &str, locale: &str) -> Result<Self, SpellError> {
        let (try_chars, sfx_rules, pfx_rules) = parse_aff(aff);
        let base_words = parse_dic(dic);
        if base_words.is_empty() {
            return Err(SpellError::EmptyDictionary);
        }
        let (words, words_lower) = expand_words(base_words, &sfx_rules, &pfx_rules);
        Ok(Self {
            words,
            words_lower,
            try_chars,
            locale: locale.to_owned(),
        })
    }
}

impl SpellChecker for HunspellDictionary {
    fn check(&self, word: &str) -> bool {
        let word = word.trim();
        if word.is_empty() {
            return true;
        }
        if !word.chars().any(|c| c.is_alphabetic()) {
            return true;
        }
        if self.words.contains(word) {
            return true;
        }
        let lower = word.to_lowercase();
        self.words_lower.contains(&lower)
    }

    fn suggest(&self, word: &str) -> Vec<String> {
        if self.check(word) {
            return Vec::new();
        }
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        // deletions
        for i in 0..len {
            let mut s = String::with_capacity(len - 1);
            s.extend(chars[..i].iter());
            s.extend(chars[i + 1..].iter());
            add_candidate(&mut candidates, &mut seen, &s, self);
        }
        // transpositions
        for i in 0..len.saturating_sub(1) {
            let mut s = String::with_capacity(len);
            s.extend(chars[..i].iter());
            s.push(chars[i + 1]);
            s.push(chars[i]);
            s.extend(chars[i + 2..].iter());
            add_candidate(&mut candidates, &mut seen, &s, self);
        }
        // replacements
        for i in 0..len {
            for &c in &self.try_chars {
                if c == chars[i] {
                    continue;
                }
                let mut s = String::with_capacity(len);
                s.extend(chars[..i].iter());
                s.push(c);
                s.extend(chars[i + 1..].iter());
                add_candidate(&mut candidates, &mut seen, &s, self);
            }
        }
        // insertions
        for i in 0..=len {
            for &c in &self.try_chars {
                let mut s = String::with_capacity(len + 1);
                s.extend(chars[..i].iter());
                s.push(c);
                s.extend(chars[i..].iter());
                add_candidate(&mut candidates, &mut seen, &s, self);
            }
        }
        candidates.truncate(8);
        candidates
    }

    fn locale(&self) -> &str {
        &self.locale
    }
}

fn add_candidate(candidates: &mut Vec<String>, seen: &mut HashSet<String>, s: &str, dict: &HunspellDictionary) {
    if !s.is_empty() && dict.check(s) && seen.insert(s.to_owned()) {
        candidates.push(s.to_owned());
    }
}

/// Парсит .aff, возвращает (try_chars, sfx_rules, pfx_rules).
fn parse_aff(aff: &str) -> (Vec<char>, Vec<AffixRule>, Vec<AffixRule>) {
    let mut try_chars = Vec::new();
    let mut sfx_rules = Vec::new();
    let mut pfx_rules = Vec::new();
    let lines: Vec<&str> = aff.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "TRY" if parts.len() >= 2 => {
                try_chars = parts[1].chars().collect();
            }
            "SFX" | "PFX" => {
                if parts.len() != 4 {
                    continue;
                }
                let flag = parts[1].chars().next();
                let cross = parts[2] == "Y";
                let count = parts[3].parse::<usize>().unwrap_or(0);
                let mut rules = Vec::new();
                for _ in 0..count {
                    if i >= lines.len() {
                        break;
                    }
                    let rule_line = lines[i].trim();
                    i += 1;
                    if rule_line.is_empty() || rule_line.starts_with('#') {
                        continue;
                    }
                    let rparts: Vec<&str> = rule_line.split_whitespace().collect();
                    if rparts.len() < 4 || rparts[0] != parts[0] {
                        continue;
                    }
                    if let Some(flag) = flag {
                        let strip = if rparts[2] == "0" { String::new() } else { rparts[2].to_owned() };
                        let add = rparts[3].split('/').next().unwrap_or("").to_owned();
                        let condition = rparts.get(4).copied().unwrap_or(".").to_owned();
                        rules.push(AffixRule {
                            flag,
                            cross_product: cross,
                            strip,
                            add,
                            condition,
                        });
                    }
                }
                match parts[0] {
                    "SFX" => sfx_rules.extend(rules),
                    "PFX" => pfx_rules.extend(rules),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    (try_chars, sfx_rules, pfx_rules)
}

/// Парсит .dic, возвращает вектор пар (слово, строка_флагов).
fn parse_dic(dic: &str) -> Vec<(String, String)> {
    let mut base_words = Vec::new();
    let lines: Vec<&str> = dic.lines().collect();
    let mut i = 0;
    if let Some(first) = lines.first()
        && first.trim().parse::<usize>().is_ok()
    {
        i = 1;
    }
    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() {
            continue;
        }
        let line = line.split('\t').next().unwrap_or("");
        let (word, flags) = line.split_once('/').unwrap_or((line, ""));
        if !word.is_empty() {
            base_words.push((word.to_owned(), flags.to_owned()));
        }
    }
    base_words
}

#[derive(Debug, Clone)]
struct AffixRule {
    flag: char,
    cross_product: bool,
    strip: String,
    add: String,
    condition: String,
}

/// Раскрывает все формы слов.
fn expand_words(
    base_words: Vec<(String, String)>,
    sfx_rules: &[AffixRule],
    pfx_rules: &[AffixRule],
) -> (HashSet<String>, HashSet<String>) {
    let mut words = HashSet::new();
    let mut words_lower = HashSet::new();

    // Group rules by flag for quick lookup
    let mut sfx_by_flag: HashMap<char, Vec<&AffixRule>> = HashMap::new();
    let mut pfx_by_flag: HashMap<char, Vec<&AffixRule>> = HashMap::new();
    for rule in sfx_rules {
        sfx_by_flag.entry(rule.flag).or_default().push(rule);
    }
    for rule in pfx_rules {
        pfx_by_flag.entry(rule.flag).or_default().push(rule);
    }

    for (word, flags) in base_words {
        insert_word(&mut words, &mut words_lower, &word);

        // Collect SFX-derived forms with their cross_product flag
        let mut sfx_derived: Vec<(String, bool)> = Vec::new();
        for flag in flags.chars() {
            if let Some(rules) = sfx_by_flag.get(&flag) {
                for rule in rules {
                    if condition_matches(&word, &rule.condition, true)
                        && let Some(derived) = apply_sfx(&word, rule)
                    {
                        insert_word(&mut words, &mut words_lower, &derived);
                        sfx_derived.push((derived, rule.cross_product));
                    }
                }
            }
        }

        // PFX forms from base
        for flag in flags.chars() {
            if let Some(rules) = pfx_by_flag.get(&flag) {
                for rule in rules {
                    if condition_matches(&word, &rule.condition, false)
                        && let Some(derived) = apply_pfx(&word, rule)
                    {
                        insert_word(&mut words, &mut words_lower, &derived);
                    }
                }
            }
        }

        // Cross product: PFX applied to SFX-derived forms
        for flag_pfx in flags.chars() {
            if let Some(pfx_rules_for_flag) = pfx_by_flag.get(&flag_pfx) {
                for pfx_rule in pfx_rules_for_flag {
                    if !pfx_rule.cross_product {
                        continue;
                    }
                    if !condition_matches(&word, &pfx_rule.condition, false) {
                        continue;
                    }
                    for (sfx_form, sfx_cross) in &sfx_derived {
                        if *sfx_cross
                            && let Some(derived) = apply_pfx(sfx_form, pfx_rule)
                        {
                            insert_word(&mut words, &mut words_lower, &derived);
                        }
                    }
                }
            }
        }
    }

    (words, words_lower)
}

fn insert_word(words: &mut HashSet<String>, words_lower: &mut HashSet<String>, word: &str) {
    words.insert(word.to_owned());
    words_lower.insert(word.to_lowercase());
}

fn apply_sfx(word: &str, rule: &AffixRule) -> Option<String> {
    let strip = &rule.strip;
    let add = &rule.add;
    if strip.is_empty() {
        Some(format!("{}{}", word, add))
    } else if word.ends_with(strip) {
        Some(format!("{}{}", &word[..word.len() - strip.len()], add))
    } else {
        None
    }
}

fn apply_pfx(word: &str, rule: &AffixRule) -> Option<String> {
    let strip = &rule.strip;
    let add = &rule.add;
    if strip.is_empty() {
        Some(format!("{}{}", add, word))
    } else if word.starts_with(strip) {
        Some(format!("{}{}", add, &word[strip.len()..]))
    } else {
        None
    }
}

/// Условие: последовательность элементов.
#[derive(Debug, Clone, PartialEq)]
enum CondElem {
    Any,
    Literal(char),
    CharSet(Vec<char>, bool), // chars, negated
}

fn parse_condition(cond: &str) -> Vec<CondElem> {
    let mut elems = Vec::new();
    let mut chars = cond.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => elems.push(CondElem::Any),
            '[' => {
                let mut set = Vec::new();
                let mut negated = false;
                if let Some(&'^') = chars.peek() {
                    negated = true;
                    chars.next();
                }
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    set.push(c);
                }
                elems.push(CondElem::CharSet(set, negated));
            }
            _ => elems.push(CondElem::Literal(c)),
        }
    }
    elems
}

fn condition_matches(word: &str, condition: &str, is_suffix: bool) -> bool {
    let pattern = parse_condition(condition);
    let word_chars: Vec<char> = word.chars().collect();
    let k = pattern.len();
    if word_chars.len() < k {
        return false;
    }
    let slice = if is_suffix {
        &word_chars[word_chars.len() - k..]
    } else {
        &word_chars[..k]
    };
    for (elem, &ch) in pattern.iter().zip(slice.iter()) {
        match elem {
            CondElem::Any => {}
            CondElem::Literal(c) => {
                if *c != ch {
                    return false;
                }
            }
            CondElem::CharSet(set, negated) => {
                let contains = set.contains(&ch);
                if contains == *negated {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = r#"
SET UTF-8
TRY esianrtolcdugmphbyfvkwz
SFX D Y 2
SFX D   0   ed   [^e]
SFX D   e   ed   e
PFX U Y 1
PFX U   0   un   .
"#;

    const DIC: &str = r#"
4
walk/D
smile/D
lock/DU
привет
"#;

    fn dict() -> HunspellDictionary {
        HunspellDictionary::from_aff_dic(AFF, DIC, "en-US").unwrap()
    }

    #[test]
    fn check_base_words() {
        let d = dict();
        assert!(d.check("walk"));
        assert!(d.check("привет"));
        assert!(!d.check("walkz"));
    }

    #[test]
    fn sfx_expansion() {
        let d = dict();
        assert!(d.check("walked"));
        assert!(d.check("smiled"));
        assert!(!d.check("smileed"));
    }

    #[test]
    fn pfx_expansion() {
        let d = dict();
        assert!(d.check("unlock"));
    }

    #[test]
    fn cross_product() {
        let d = dict();
        assert!(d.check("unlocked"));
    }

    #[test]
    fn case_insensitive() {
        let d = dict();
        assert!(d.check("Привет"));
        assert!(d.check("WALK"));
    }

    #[test]
    fn non_alphabetic_passes() {
        let d = dict();
        assert!(d.check("123"));
        assert!(d.check("..."));
        assert!(d.check(""));
    }

    #[test]
    fn suggest_basic() {
        let d = dict();
        let s = d.suggest("walkk");
        assert!(s.contains(&"walk".to_owned()));
        assert!(d.suggest("walk").is_empty());
    }

    #[test]
    fn suggest_cap() {
        let d = dict();
        // generate many candidates by using a word with many possible edits
        let s = d.suggest("aaaaaaaaaa");
        assert!(s.len() <= 8);
    }

    #[test]
    fn empty_dic_error() {
        let err = HunspellDictionary::from_aff_dic("", "", "en").unwrap_err();
        assert_eq!(err, SpellError::EmptyDictionary);
    }

    #[test]
    fn locale_passthrough() {
        let d = HunspellDictionary::from_aff_dic(AFF, DIC, "ru-RU").unwrap();
        assert_eq!(d.locale(), "ru-RU");
    }

    #[test]
    fn condition_negated_set() {
        // free/D: rule [^e] should not match (ends with e), rule e->ed gives freed
        let aff = r#"
TRY abcdefghijklmnopqrstuvwxyz
SFX D Y 2
SFX D 0 ed [^e]
SFX D e ed e
"#;
        let dic = "1\nfree/D";
        let d = HunspellDictionary::from_aff_dic(aff, dic, "en").unwrap();
        assert!(d.check("freed"));
        assert!(!d.check("freeed"));
    }
}
