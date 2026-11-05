//! WebVTT track loading for `<video>` (P3-webvtt slice 3).
//!
//! Pure logic layer: fetching is abstracted behind a closure so the module is
//! unit-testable without network. Since VTT-RENDER-S5 (BUG-1748) this module
//! only *feeds* the JS side (`video.textTracks` via the mirrored
//! `TextTrackStore`); cue rendering runs through the JS→DOM bridge
//! (`__lumen_video_render_cues`) as CSS boxes in the UA shadow — the old
//! shell-side display-list overlay is gone.

use std::collections::HashMap;

use lumen_dom::vtt::{TrackInfo, VttCue, collect_video_tracks, parse_vtt};
use lumen_dom::{Document, NodeId};

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

/// Загруженные треки по каждому `<video>` страницы (VTT-RENDER-S5: корм для
/// JS; рендер cue — через `__lumen_video_render_cues`, не отсюда).
#[derive(Debug, Default)]
pub struct PageTracks {
    /// NodeId элемента `<video>` -> все его треки (для `video.textTracks`).
    pub tracks_by_video: HashMap<NodeId, Vec<LoadedTrack>>,
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
    }
    result
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
        let showing: Vec<_> = tracks.tracks_by_video[&video]
            .iter()
            .filter(|t| t.mode == "showing")
            .collect();
        assert_eq!(showing.len(), 1);
        assert_eq!(showing[0].cues.len(), 1);
        assert_eq!(showing[0].cues[0].text, "Default");
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
        let showing: Vec<_> = tracks.tracks_by_video[&video]
            .iter()
            .filter(|t| t.mode == "showing")
            .collect();
        assert_eq!(showing.len(), 1);
        assert_eq!(showing[0].cues.len(), 1);
        assert_eq!(showing[0].cues[0].text, "Captions");
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
        let list = tracks.tracks_by_video.get(&video);
        assert!(
            list.is_none() || list.unwrap().iter().all(|t| t.mode == "disabled"),
            "fetch failure leaves the video without a showing track",
        );
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
        let list = tracks.tracks_by_video.get(&video);
        assert!(
            list.is_none() || list.unwrap().iter().all(|t| t.mode == "disabled"),
            "invalid VTT leaves the video without a showing track",
        );
    }


}
