//! P1/SPLIT-DL1: тесты CSS Custom Highlight API L1 хелпера
//! (`emit_text_with_highlights`) + отдельно стоящие тесты
//! text-decoration-skip-ink, физически оказавшиеся ПОСЛЕ закрывающей скобки
//! `mod highlight_tests` (не внутри неё — регион перенесён байт-в-байт,
//! структура исходника не правилась). Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-1).

use super::*;

#[cfg(test)]
mod highlight_tests {
    use super::*;

    #[test]
    fn highlight_field_none_by_default() {
        // DrawText created without highlight_name should have None
        let dl = DisplayList::from(vec![DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: Rect::new(0.0, 0.0, 100.0, 20.0),
            text: "test".to_string(),
            font_size: 14.0,
            color: Color::BLACK,
            font_family: vec![],
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        }]);
        
        if let DisplayCommand::DrawText { highlight_name, .. } = &dl[0] {
            assert!(highlight_name.is_none());
        }
    }

    #[test]
    fn emit_text_with_highlights_creates_command() {
        let mut out = Vec::new();
        emit_text_with_highlights(
            Rect::new(10.0, 20.0, 100.0, 30.0),
            "highlighted",
            16.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            FontStretch::NORMAL,
            vec![],
            Vec::new(),
            0.0,
            Some("search".to_string()),
            None,
            &mut out,
        );
        
        assert_eq!(out.len(), 1);
        if let DisplayCommand::DrawText { text, highlight_name, .. } = &out[0] {
            assert_eq!(text, "highlighted");
            assert_eq!(highlight_name.as_ref(), Some(&"search".to_string()));
        }
    }

    #[test]
    fn highlight_name_custom_values() {
        let names = vec!["search", "spelling", "grammar"];
        for name in names {
            let mut out = Vec::new();
            emit_text_with_highlights(
                Rect::new(0.0, 0.0, 50.0, 20.0),
                "test",
                12.0,
                Color::BLACK,
                vec![],
                FontWeight::NORMAL,
                FontStyle::Normal,
                FontStretch::NORMAL,
                vec![],
                Vec::new(),
                0.0,
                Some(name.to_string()),
                None,
                &mut out,
            );
            
            if let DisplayCommand::DrawText { highlight_name, .. } = &out[0] {
                assert_eq!(highlight_name.as_ref(), Some(&name.to_string()));
            }
        }
    }

    #[test]
    fn highlight_without_name() {
        let mut out = Vec::new();
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 50.0, 20.0),
            "plain",
            14.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            FontStretch::NORMAL,
            vec![],
            Vec::new(),
            0.0,
            None,
            None,
            &mut out,
        );
        
        assert_eq!(out.len(), 1);
        if let DisplayCommand::DrawText { highlight_name, .. } = &out[0] {
            assert!(highlight_name.is_none());
        }
    }

    #[test]
    fn highlight_preserves_text_attributes() {
        let mut out = Vec::new();
        let family = vec!["Arial".to_string()];
        let weight = FontWeight(600);
        
        emit_text_with_highlights(
            Rect::new(5.0, 10.0, 200.0, 25.0),
            "styled",
            18.0,
            Color::BLACK,
            family.clone(),
            weight,
            FontStyle::Italic,
            FontStretch::NORMAL,
            vec![],
            Vec::new(),
            4.0,
            Some("custom".to_string()),
            None,
            &mut out,
        );
        
        if let DisplayCommand::DrawText {
            text, font_size, font_family, font_weight, font_style,
            highlight_name, tab_size, ..
        } = &out[0] {
            assert_eq!(text, "styled");
            assert_eq!(*font_size, 18.0);
            assert_eq!(*font_family, family);
            assert_eq!(*font_weight, weight);
            assert_eq!(*font_style, FontStyle::Italic);
            assert_eq!(highlight_name.as_ref(), Some(&"custom".to_string()));
            assert_eq!(*tab_size, 4.0);
        }
    }

    #[test]
    fn highlight_empty_text() {
        let mut out = Vec::new();
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 0.0, 0.0),
            "",
            12.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            FontStretch::NORMAL,
            vec![],
            Vec::new(),
            0.0,
            Some("empty".to_string()),
            None,
            &mut out,
        );
        
        assert_eq!(out.len(), 1);
        if let DisplayCommand::DrawText { text, highlight_name, .. } = &out[0] {
            assert_eq!(text, "");
            assert_eq!(highlight_name.as_ref(), Some(&"empty".to_string()));
        }
    }

    #[test]
    fn highlight_multiple_independent_calls() {
        let mut out1 = Vec::new();
        let mut out2 = Vec::new();
        
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 100.0, 20.0),
            "first",
            14.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            FontStretch::NORMAL,
            vec![],
            Vec::new(),
            0.0,
            Some("search".to_string()),
            None,
            &mut out1,
        );
        
        emit_text_with_highlights(
            Rect::new(0.0, 20.0, 100.0, 20.0),
            "second",
            14.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            FontStretch::NORMAL,
            vec![],
            Vec::new(),
            0.0,
            Some("spelling".to_string()),
            None,
            &mut out2,
        );
        
        if let (
            DisplayCommand::DrawText { text: t1, highlight_name: h1, .. },
            DisplayCommand::DrawText { text: t2, highlight_name: h2, .. },
        ) = (&out1[0], &out2[0])
        {
            assert_eq!(t1, "first");
            assert_eq!(h1.as_ref(), Some(&"search".to_string()));
            assert_eq!(t2, "second");
            assert_eq!(h2.as_ref(), Some(&"spelling".to_string()));
        }
    }
}

    #[test]
    fn highlight_with_variation_axes() {
        let mut out = Vec::new();
        let axes = vec![((*b"wght"), 600.0)];
        
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 100.0, 20.0),
            "variable",
            16.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            FontStretch::NORMAL,
            axes.clone(),
            Vec::new(),
            0.0,
            Some("variable-font".to_string()),
            None,
            &mut out,
        );
        
        if let DisplayCommand::DrawText { font_variation_axes, highlight_name, .. } = &out[0] {
            assert_eq!(font_variation_axes, &axes);
            assert_eq!(highlight_name.as_ref(), Some(&"variable-font".to_string()));
        }
    }

    // ── text-decoration-skip-ink ──────────────────────────────────────────────

    #[test]
    fn skip_ink_auto_no_descenders_single_segment() {
        // skip-ink: auto on text without descenders → single contiguous FillRect.
        let mut out = Vec::new();
        emit_decoration_line_skip_ink(&mut out, SkipInkParams {
            x: 0.0, y: 10.0, width: 100.0, thickness: 1.0, color: Color::BLACK,
            style: TextDecorationStyle::Solid, text: "art", skip_all: false,
        });
        let count = out.iter().filter(|c| matches!(c, DisplayCommand::FillRect { .. })).count();
        // 'a', 'r', 't' have no descenders — full line, single rect.
        assert_eq!(count, 1);
    }

    #[test]
    fn skip_ink_auto_gaps_for_descenders() {
        // skip-ink: auto on "xpx": 'x' has no descender, 'p' has one.
        // Expected: two segments flanking the gap around 'p'.
        let mut out_descender = Vec::new();
        emit_decoration_line_skip_ink(&mut out_descender, SkipInkParams {
            x: 0.0, y: 10.0, width: 90.0, thickness: 1.0, color: Color::BLACK,
            style: TextDecorationStyle::Solid, text: "xpx", skip_all: false,
        });
        // skip-ink: auto on "abc" (no descenders) → one continuous FillRect.
        let mut out_plain = Vec::new();
        emit_decoration_line_skip_ink(&mut out_plain, SkipInkParams {
            x: 0.0, y: 10.0, width: 90.0, thickness: 1.0, color: Color::BLACK,
            style: TextDecorationStyle::Solid, text: "abc", skip_all: false,
        });
        let count_descender = out_descender.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { .. })
        }).count();
        let count_plain = out_plain.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { .. })
        }).count();
        // "xpx": gap around 'p' splits the line into two segments.
        assert!(count_descender >= 2, "expected ≥2 segments around 'p' gap, got {count_descender}");
        // "abc": no gaps → single segment.
        assert_eq!(count_plain, 1);
    }

    #[test]
    fn skip_ink_all_gaps_for_every_char() {
        // skip-ink: all → skip_all=true → every character gets a gap.
        let mut out = Vec::new();
        emit_decoration_line_skip_ink(&mut out, SkipInkParams {
            x: 0.0, y: 10.0, width: 60.0, thickness: 1.0, color: Color::BLACK,
            style: TextDecorationStyle::Solid, text: "abc", skip_all: true,
        });
        // With 3 chars each getting a gap, segments are drawn only between/around cells.
        // The total painted width must be strictly less than the full 60px.
        let total: f32 = out.iter().filter_map(|c| {
            if let DisplayCommand::FillRect { rect, .. } = c { Some(rect.width) } else { None }
        }).sum();
        assert!(total < 60.0, "expected gaps to reduce total painted width, got {total}");
    }

    #[test]
    fn skip_ink_consecutive_descenders_keep_line_visible() {
        // BUG-203: a run of consecutive descenders ("gjpqy") must keep the
        // underline visible as segments between glyphs. The old full-cell + margin
        // gap merged the whole run into one giant gap, erasing the line entirely.
        let mut out = Vec::new();
        emit_decoration_line_skip_ink(&mut out, SkipInkParams {
            x: 0.0, y: 10.0, width: 100.0, thickness: 3.0, color: Color::BLACK,
            style: TextDecorationStyle::Solid, text: "gjpqy", skip_all: false,
        });
        let segments: Vec<f32> = out.iter().filter_map(|c| {
            if let DisplayCommand::FillRect { rect, .. } = c { Some(rect.width) } else { None }
        }).collect();
        // Line must NOT be erased: at least one segment survives between glyphs.
        assert!(!segments.is_empty(), "underline erased for consecutive descenders");
        // Multiple inter-glyph segments expected, not one merged gap.
        assert!(segments.len() >= 2,
            "expected ≥2 inter-glyph segments, got {}", segments.len());
        let painted: f32 = segments.iter().sum();
        // Gaps cover only the central ink of each cell, so a substantial part of
        // the 100px line survives (the old code erased it to 0px).
        assert!(painted > 20.0,
            "expected a substantial part of the 100px line to remain, got {painted}");
    }

    #[test]
    fn skip_ink_all_does_not_erase_line() {
        // BUG-203: skip-ink: all must still draw line segments between glyphs,
        // not erase the line. Regression for the all-cells-merged-into-one-gap bug.
        let mut out = Vec::new();
        emit_decoration_line_skip_ink(&mut out, SkipInkParams {
            x: 0.0, y: 10.0, width: 240.0, thickness: 2.0, color: Color::BLACK,
            style: TextDecorationStyle::Solid, text: "Typography", skip_all: true,
        });
        let count = out.iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .count();
        assert!(count >= 2, "skip-ink: all erased the line, got {count} segments");
    }

    #[test]
    fn char_has_ink_descender_common_cases() {
        assert!(char_has_ink_descender('g'));
        assert!(char_has_ink_descender('j'));
        assert!(char_has_ink_descender('p'));
        assert!(char_has_ink_descender('q'));
        assert!(char_has_ink_descender('y'));
        assert!(char_has_ink_descender('Q'));
        assert!(char_has_ink_descender('J'));
        // non-descenders
        assert!(!char_has_ink_descender('a'));
        assert!(!char_has_ink_descender('e'));
        assert!(!char_has_ink_descender('m'));
        assert!(!char_has_ink_descender('x'));
        assert!(!char_has_ink_descender('z'));
    }
