//! WebVTT track loading and active-cue overlay for `<video>` (P3-webvtt slice 3).
//!
//! Pure logic layer: fetching is abstracted behind a closure so the module is
//! unit-testable without network; painting produces plain `DisplayCommand`s
//! that the shell appends to the overlay display list.

use std::collections::HashMap;

use lumen_core::geom::Rect;
use lumen_dom::vtt::{
    CueTextAlign, TrackInfo, VttCue, active_cues, collect_video_tracks, parse_vtt,
    resolve_cue_box, strip_cue_markup,
};
use lumen_dom::{Document, NodeId};
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::DisplayCommand;

/// Один `<track>` элемента `<video>`, отражённый в `TextTrack` JS-API.
///
/// Хранит метаданные всех треков (для перечисления `video.textTracks`) и cues
/// только у показываемого («showing») трека — остальные `disabled` без cues,
/// как и в спецификации (cues грузятся только при `mode != disabled`).
#[derive(Debug, Clone)]
pub struct LoadedTrack {
    /// `TextTrack.kind` (`subtitles`/`captions`/`chapters`/…).
    pub kind: String,
    /// `TextTrack.label`.
    pub label: String,
    /// `TextTrack.language` (из атрибута `srclang`).
    pub language: String,
    /// `TextTrack.mode`: `"showing"` у выбранного трека, иначе `"disabled"`.
    pub mode: String,
    /// Разобранные cues; непустые только у показываемого трека.
    pub cues: Vec<VttCue>,
}

/// Загруженные cues по каждому `<video>` страницы.
#[derive(Debug, Default)]
pub struct PageTracks {
    /// NodeId элемента `<video>` -> cues выбранного `<track>` (для оверлея).
    pub cues_by_video: HashMap<NodeId, Vec<VttCue>>,
    /// NodeId элемента `<video>` -> все его треки (для `video.textTracks`).
    pub tracks_by_video: HashMap<NodeId, Vec<LoadedTrack>>,
}

impl PageTracks {
    /// Нет ни одного видео с загруженными cues.
    pub fn is_empty(&self) -> bool {
        self.cues_by_video.is_empty()
    }
}

/// Индекс подходящего `<track>` для видео: приоритет — первый с `default == true`
/// среди kind "subtitles"/"captions", иначе первый подходящий по kind.
fn choose_track_index(tracks: &[TrackInfo]) -> Option<usize> {
    let is_subs = |t: &TrackInfo| {
        let kind = t.kind.to_lowercase();
        kind == "subtitles" || kind == "captions"
    };
    if let Some(i) = tracks.iter().position(|t| is_subs(t) && t.default) {
        return Some(i);
    }
    tracks.iter().position(is_subs)
}

/// Обходит документ, для каждого `<video>` выбирает один `<track>` для оверлея,
/// а также отражает метаданные всех треков для `TextTrack` JS-API.
pub fn load_video_tracks(
    doc: &Document,
    fetch: &dyn Fn(&str) -> Option<String>,
) -> PageTracks {
    let mut result = PageTracks::default();
    let video_tracks = collect_video_tracks(doc);
    for vt in video_tracks {
        if vt.tracks.is_empty() {
            continue;
        }
        let chosen = choose_track_index(&vt.tracks);
        // Грузим cues выбранного трека один раз.
        let chosen_cues = chosen.and_then(|i| {
            let text = fetch(&vt.tracks[i].src)?;
            match parse_vtt(&text) {
                Ok(c) if !c.is_empty() => Some(c),
                _ => None,
            }
        });

        let mut loaded = Vec::with_capacity(vt.tracks.len());
        for (i, ti) in vt.tracks.iter().enumerate() {
            let is_showing = chosen == Some(i) && chosen_cues.is_some();
            let cues = if is_showing {
                chosen_cues.clone().unwrap_or_default()
            } else {
                Vec::new()
            };
            loaded.push(LoadedTrack {
                kind: ti.kind.clone(),
                label: ti.label.clone(),
                language: ti.srclang.clone(),
                mode: if is_showing { "showing" } else { "disabled" }.to_string(),
                cues,
            });
        }
        result.tracks_by_video.insert(vt.video, loaded);

        if let (Some(_), Some(cues)) = (chosen, chosen_cues) {
            result.cues_by_video.insert(vt.video, cues);
        }
    }
    result
}

/// Строит оверлей активных cue. Время воспроизведения каждого видео
/// вычисляется замыканием `time_for` (секунды): для GIF-видео — реальный
/// playback-клок из `VideoGifStore`, иначе фолбэк на время от навигации.
pub fn build_cue_overlay(
    tracks: &PageTracks,
    video_rects: &[(NodeId, Rect)],
    time_for: &dyn Fn(NodeId) -> f64,
    measure: &dyn Fn(&str, f32) -> f32,
) -> Vec<DisplayCommand> {
    let rect_map: HashMap<NodeId, Rect> = video_rects.iter().cloned().collect();
    let mut commands = Vec::new();

    for (&video_id, cues) in &tracks.cues_by_video {
        let &rect = match rect_map.get(&video_id) {
            Some(r) => r,
            None => continue,
        };

        let t = time_for(video_id);
        let font_size = (rect.height * 0.06).clamp(12.0, 26.0);
        let line_height = font_size * 1.3;
        let pad = font_size * 0.3;
        let active = active_cues(cues, t);
        let mut auto_offset = 0.0;

        for cue in active {
            let raw_text = strip_cue_markup(&cue.text);
            let lines: Vec<&str> = raw_text
                .split('\n')
                .filter(|l| !l.is_empty())
                .collect();
            if lines.is_empty() {
                continue;
            }

            let cue_box = resolve_cue_box(&cue.settings, rect.x, rect.y, rect.width, rect.height);
            let block_h = lines.len() as f32 * line_height;
            let y_top = if cue.settings.line.is_none() {
                let y = rect.y + rect.height - block_h - pad - auto_offset;
                auto_offset += block_h + pad;
                y
            } else {
                cue_box.y.clamp(rect.y, rect.y + rect.height - block_h)
            };

            for (i, line) in lines.iter().enumerate() {
                let tw = measure(line, font_size);
                let tx = match cue_box.align {
                    CueTextAlign::Start => cue_box.x,
                    CueTextAlign::Center => cue_box.x + (cue_box.w - tw) / 2.0,
                    CueTextAlign::End => cue_box.x + cue_box.w - tw,
                }
                .max(rect.x);

                let ly = y_top + i as f32 * line_height;

                // Подложка под строку
                commands.push(DisplayCommand::FillRect {
                    rect: Rect::new(tx - pad, ly, tw + 2.0 * pad, line_height),
                    color: Color { r: 0, g: 0, b: 0, a: 170 },
                });

                // Текст строки
                commands.push(DisplayCommand::DrawText {
                    rect: Rect::new(
                        tx,
                        ly + (line_height - font_size) * 0.5,
                        tw,
                        font_size * 1.2,
                    ),
                    text: line.to_string(),
                    font_size,
                    color: Color::WHITE,
                    font_family: Vec::new(),
                    font_weight: FontWeight::NORMAL,
                    font_style: FontStyle::Normal,
                    font_variation_axes: Vec::new(),
                    font_features: Vec::new(),
                    font_palette: None,
                    tab_size: 0.0,
                    highlight_name: None,
                    text_orientation: None,
                });
            }
        }
    }

    commands
}

/// Рекурсивно собирает `(NodeId, Rect)` всех video-боксов layout-дерева
/// (координаты страницы — те же, что в display list).
pub fn collect_video_rects(lb: &lumen_layout::LayoutBox, out: &mut Vec<(NodeId, Rect)>) {
    if matches!(lb.kind, lumen_layout::BoxKind::Video { .. }) {
        out.push((lb.node, lb.rect));
    }
    for child in &lb.children {
        collect_video_rects(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_dom::{Attribute, NodeData, QualName};

    #[test]
    fn test_default_track_priority() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track_default = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track_default).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "default.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("default"),
                value: String::new(),
            });
        }
        let track_no_default = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track_no_default).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "no_default.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track_default);
        doc.append_child(video, track_no_default);
        doc.append_child(doc.root(), video);

        let fetch = |src: &str| {
            if src == "default.vtt" {
                Some("WEBVTT\n\n00:00.000 --> 00:05.000\nDefault".to_string())
            } else {
                None
            }
        };
        let tracks = load_video_tracks(&doc, &fetch);
        assert!(tracks.cues_by_video.contains_key(&video));
        let cues = tracks.cues_by_video.get(&video).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Default");
    }

    #[test]
    fn test_tracks_by_video_exposes_all_tracks() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));

        let subs = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(subs).data {
            for (k, v) in [
                ("src", "subs.vtt"),
                ("kind", "subtitles"),
                ("srclang", "en"),
                ("label", "English"),
            ] {
                attrs.push(Attribute {
                    name: QualName::html(k),
                    value: v.to_string(),
                });
            }
            attrs.push(Attribute {
                name: QualName::html("default"),
                value: String::new(),
            });
        }
        let chapters = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(chapters).data {
            for (k, v) in [("src", "chapters.vtt"), ("kind", "chapters")] {
                attrs.push(Attribute {
                    name: QualName::html(k),
                    value: v.to_string(),
                });
            }
        }
        doc.append_child(video, subs);
        doc.append_child(video, chapters);
        doc.append_child(doc.root(), video);

        let fetch = |src: &str| {
            if src == "subs.vtt" {
                Some("WEBVTT\n\n00:00.000 --> 00:05.000\nHi".to_string())
            } else {
                None
            }
        };
        let tracks = load_video_tracks(&doc, &fetch);
        let list = tracks.tracks_by_video.get(&video).unwrap();
        assert_eq!(list.len(), 2, "both <track> elements exposed");

        let showing = list.iter().find(|t| t.mode == "showing").unwrap();
        assert_eq!(showing.kind, "subtitles");
        assert_eq!(showing.language, "en");
        assert_eq!(showing.label, "English");
        assert_eq!(showing.cues.len(), 1);

        let chap = list.iter().find(|t| t.kind == "chapters").unwrap();
        assert_eq!(chap.mode, "disabled");
        assert!(chap.cues.is_empty(), "non-showing tracks carry no cues");
    }

    #[test]
    fn test_ignores_non_subtitle_kinds() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track_chapters = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track_chapters).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "chapters.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "chapters".to_string(),
            });
        }
        let track_captions = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track_captions).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "captions.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "captions".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("default"),
                value: String::new(),
            });
        }
        doc.append_child(video, track_chapters);
        doc.append_child(video, track_captions);
        doc.append_child(doc.root(), video);

        let fetch = |src: &str| match src {
            "chapters.vtt" => {
                Some("WEBVTT\n\n00:00.000 --> 00:05.000\nChapter 1".to_string())
            }
            "captions.vtt" => {
                Some("WEBVTT\n\n00:00.000 --> 00:05.000\nCaptions".to_string())
            }
            _ => None,
        };
        let tracks = load_video_tracks(&doc, &fetch);
        let cues = tracks.cues_by_video.get(&video).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Captions");
    }

    #[test]
    fn test_fetch_returns_none_skips_video() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "missing.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);

        let fetch = |_: &str| None;
        let tracks = load_video_tracks(&doc, &fetch);
        assert!(!tracks.cues_by_video.contains_key(&video));
    }

    #[test]
    fn test_invalid_vtt_skipped() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "invalid.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);

        let fetch = |_: &str| Some("Not a valid VTT".to_string());
        let tracks = load_video_tracks(&doc, &fetch);
        assert!(!tracks.cues_by_video.contains_key(&video));
    }

    #[test]
    fn test_no_active_cues_empty_overlay() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "subs.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);

        let fetch = |_: &str| {
            Some("WEBVTT\n\n00:00.000 --> 00:01.000\nHello".to_string())
        };
        let tracks = load_video_tracks(&doc, &fetch);
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let measure = |_: &str, _: f32| 100.0;
        let overlay = build_cue_overlay(&tracks, &[(video, rect)], &|_| 2.0, &measure);
        assert!(overlay.is_empty());
    }

    #[test]
    fn test_active_cue_commands_strips_markup() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "subs.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);

        let fetch = |_: &str| {
            Some("WEBVTT\n\n00:00.000 --> 00:05.000\n<b>Привет</b>".to_string())
        };
        let tracks = load_video_tracks(&doc, &fetch);
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let measure = |s: &str, fs: f32| s.chars().count() as f32 * fs * 0.5;
        let overlay = build_cue_overlay(&tracks, &[(video, rect)], &|_| 0.0, &measure);

        assert_eq!(overlay.len(), 2);
        match &overlay[0] {
            DisplayCommand::FillRect { .. } => {}
            _ => panic!("Ожидается FillRect первой командой"),
        }
        match &overlay[1] {
            DisplayCommand::DrawText { text, .. } => assert_eq!(text, "Привет"),
            _ => panic!("Ожидается DrawText второй командой"),
        }
    }

    #[test]
    fn test_center_alignment_tx() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "subs.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);

        let fetch = |_: &str| {
            Some("WEBVTT\n\n00:00.000 --> 00:05.000 align:center\nТест".to_string())
        };
        let tracks = load_video_tracks(&doc, &fetch);
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let measure = |_: &str, _: f32| 100.0;
        let overlay = build_cue_overlay(&tracks, &[(video, rect)], &|_| 0.0, &measure);

        let draw_text = overlay
            .iter()
            .find_map(|cmd| {
                if let DisplayCommand::DrawText { rect, .. } = cmd {
                    Some(rect)
                } else {
                    None
                }
            })
            .unwrap();
        // cue_box.w = 400, cue_box.x = 0, tw=100 → tx = 0 + (400-100)/2 = 150
        assert!((draw_text.x - 150.0).abs() < 0.01);
    }

    #[test]
    fn test_two_line_cue_two_draw_texts() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "subs.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);

        let fetch = |_: &str| {
            Some("WEBVTT\n\n00:00.000 --> 00:05.000\nСтрока1\nСтрока2".to_string())
        };
        let tracks = load_video_tracks(&doc, &fetch);
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let measure = |_: &str, _: f32| 50.0;
        let overlay = build_cue_overlay(&tracks, &[(video, rect)], &|_| 0.0, &measure);
        // Expect 2 FillRects + 2 DrawTexts = 4 commands
        assert_eq!(overlay.len(), 4);
        // Проверяем y второй строки: ly = y_top + line_height
        let draw_rects: Vec<_> = overlay
            .iter()
            .filter_map(|cmd| {
                if let DisplayCommand::DrawText { rect, .. } = cmd {
                    Some(rect)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(draw_rects.len(), 2);
        let line_height = (rect.height * 0.06).clamp(12.0, 26.0) * 1.3;
        assert!((draw_rects[1].y - draw_rects[0].y - line_height).abs() < 0.01);
    }

    #[test]
    fn test_two_active_auto_cues_stacking() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
            attrs.push(Attribute {
                name: QualName::html("src"),
                value: "subs.vtt".to_string(),
            });
            attrs.push(Attribute {
                name: QualName::html("kind"),
                value: "subtitles".to_string(),
            });
        }
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);

        let vtt = "WEBVTT\n\n00:00.000 --> 00:05.000\nПервый\n\n00:00.000 --> 00:05.000\nВторой";
        let fetch = |_: &str| Some(vtt.to_string());
        let tracks = load_video_tracks(&doc, &fetch);
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let measure = |_: &str, _: f32| 50.0;
        let overlay = build_cue_overlay(&tracks, &[(video, rect)], &|_| 0.0, &measure);
        let draw_rects: Vec<_> = overlay
            .iter()
            .filter_map(|cmd| {
                if let DisplayCommand::DrawText { rect, .. } = cmd {
                    Some(rect)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(draw_rects.len(), 2);
        // Второй cue (нижний по y) должен быть выше (меньший y), т.к. авто-штабелизация ВВЕРХ
        assert!(draw_rects[1].y < draw_rects[0].y);
    }

    #[test]
    fn test_video_not_in_tracks_skipped() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        doc.append_child(doc.root(), video);

        let tracks = PageTracks::default();
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let measure = |_: &str, _: f32| 50.0;
        let overlay = build_cue_overlay(&tracks, &[(video, rect)], &|_| 0.0, &measure);
        assert!(overlay.is_empty());
    }
}
