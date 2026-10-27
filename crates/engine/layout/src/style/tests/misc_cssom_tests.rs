//! Тесты `canonical_specified_misc` — каноническая запись `content`, `font-family`, `font-size`,
//! `list-style-image` в inline-`style` CSSOM (BUG-1551).

    use crate::style::{canonical_specified_background, canonical_specified_misc, canonical_specified_text};

    fn canon(prop: &str, v: &str) -> Option<String> {
        canonical_specified_misc(prop, v)
    }

    #[test]
    fn url_gets_double_quotes() {
        assert_eq!(canon("list-style-image", "url(http://localhost/)").as_deref(), Some("url(\"http://localhost/\")"));
        assert_eq!(canon("list-style-image", "url('a b.png')").as_deref(), Some("url(\"a b.png\")"));
        assert_eq!(canon("list-style-image", "none").as_deref(), Some("none"));
        assert_eq!(
            canonical_specified_background("background-image", "url(a.png), none").as_deref(),
            Some("url(\"a.png\"), none")
        );
    }

    #[test]
    fn content_strings_and_counters() {
        assert_eq!(canon("content", "'string'").as_deref(), Some("\"string\""));
        assert_eq!(canon("content", "'a\"b'").as_deref(), Some("\"a\\\"b\""));
        assert_eq!(canon("content", "url(http://localhost/)").as_deref(), Some("url(\"http://localhost/\")"));
        assert_eq!(canon("content", "counter(par-num, decimal)").as_deref(), Some("counter(par-num)"));
        assert_eq!(canon("content", "counter(a, upper-roman)").as_deref(), Some("counter(a, upper-roman)"));
        assert_eq!(canon("content", "counters(a, '.', decimal) 'x'").as_deref(), Some("counters(a, \".\") \"x\""));
        assert_eq!(canon("content", "NORMAL").as_deref(), Some("normal"));
        assert_eq!(canon("content", "attr(title) / 'alt'").as_deref(), Some("attr(title) / \"alt\""));
    }

    #[test]
    fn font_family_quotes_only_when_needed() {
        assert_eq!(canon("font-family", "'Lucida Grande'").as_deref(), Some("Lucida Grande"));
        assert_eq!(canon("font-family", "Twisty   Tie").as_deref(), Some("Twisty Tie"));
        assert_eq!(canon("font-family", "'34J'").as_deref(), Some("\"34J\""));
        assert_eq!(canon("font-family", "'A  B'").as_deref(), Some("\"A  B\""));
        assert_eq!(canon("font-family", "\"serif\"").as_deref(), Some("\"serif\""));
        assert_eq!(canon("font-family", "'initial'").as_deref(), Some("\"initial\""));
        assert_eq!(canon("font-family", "serif").as_deref(), Some("serif"));
        assert_eq!(
            canon("font-family", "Twisty Tie, '34J', \"serif\", Veronica, sans-serif").as_deref(),
            Some("Twisty Tie, \"34J\", \"serif\", Veronica, sans-serif")
        );
    }

    #[test]
    fn font_size_leading_zero_and_keywords() {
        assert_eq!(canon("font-size", ".1em").as_deref(), Some("0.1em"));
        assert_eq!(canon("font-size", ".5%").as_deref(), Some("0.5%"));
        assert_eq!(canon("font-size", "LARGER").as_deref(), Some("larger"));
        assert_eq!(canon("font-size", "-1px"), None);
        // Единица сохраняется как написана: `10pt` не превращается в `13.333333px`.
        assert_eq!(canon("font-size", "10pt").as_deref(), Some("10pt"));
        assert_eq!(canon("font-size", "12.50px").as_deref(), Some("12.5px"));
        assert_eq!(canon("font-size", "calc(1px + 2px)").as_deref(), Some("calc(3px)"));
    }

    #[test]
    fn line_height_negative_zero() {
        assert_eq!(canonical_specified_text("line-height", "-0").as_deref(), Some("0"));
    }
