//! WebVTT parser and track collection.

use crate::{Document, NodeId};

/// Настройки позиционирования cue (WebVTT §6.3). Phase 0: сырые строки значений.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VttCueSettings {
    pub vertical: Option<String>,
    pub line: Option<String>,
    pub position: Option<String>,
    pub size: Option<String>,
    pub align: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VttCue {
    pub id: Option<String>,
    /// Начало показа, секунды.
    pub start_s: f64,
    /// Конец показа, секунды.
    pub end_s: f64,
    pub settings: VttCueSettings,
    /// Текст cue; многострочный payload склеен через '\n'. Разметку (<v>, <b>…) не трогаем.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VttError {
    /// Файл не начинается с "WEBVTT"-заголовка.
    MissingHeader,
}

impl std::fmt::Display for VttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VttError::MissingHeader => write!(f, "missing WEBVTT header"),
        }
    }
}

impl std::error::Error for VttError {}

/// Разбирает WebVTT-текст в список cues.
pub fn parse_vtt(input: &str) -> Result<Vec<VttCue>, VttError> {
    // Убираем BOM, нормализуем переводы строк.
    let s = input
        .strip_prefix('\u{FEFF}')
        .unwrap_or(input)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut lines = s.split('\n');

    let header = lines.next().unwrap_or("");
    if !(header.starts_with("WEBVTT") && header.len() >= 6 && (header.len() == 6 || header.as_bytes()[6].is_ascii_whitespace())) {
        return Err(VttError::MissingHeader);
    }

    let rest: String = lines.collect::<Vec<_>>().join("\n");

    // Разбиваем на блоки по пустым строкам.
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in rest.split('\n') {
        if line.is_empty() {
            if !current.is_empty() {
                blocks.push(current);
                current = Vec::new();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    let mut cues = Vec::new();
    for block in blocks.into_iter() {
        if block.is_empty() {
            continue;
        }
        let first = block[0];
        if (first == "NOTE" || first.starts_with("NOTE ") || first.starts_with("NOTE\t"))
            || (first == "STYLE" || first.starts_with("STYLE ") || first.starts_with("STYLE\t"))
            || (first == "REGION" || first.starts_with("REGION ") || first.starts_with("REGION\t"))
        {
            continue;
        }

        let (id, timing_idx) = if first.contains("-->") {
            (None, 0)
        } else {
            (Some(first.to_owned()), 1)
        };
        if timing_idx >= block.len() {
            continue;
        }
        let timing_str = block[timing_idx];

        let Some(arrow_pos) = timing_str.find("-->") else { continue };
        let start_str = timing_str[..arrow_pos].trim();
        let after_arrow = timing_str[arrow_pos + 3..].trim();

        let (end_str, settings_str) = if let Some(space_pos) = after_arrow.find(|c: char| c.is_ascii_whitespace()) {
            let end = after_arrow[..space_pos].trim();
            let settings = after_arrow[space_pos..].trim();
            (end, settings)
        } else {
            (after_arrow, "")
        };

        let Some(start_s) = parse_timestamp(start_str) else { continue };
        let Some(end_s) = parse_timestamp(end_str) else { continue };
        if start_s >= end_s {
            continue;
        }

        let mut settings = VttCueSettings::default();
        if !settings_str.is_empty() {
            for part in settings_str.split_whitespace() {
                if let Some((key, value)) = part.split_once(':') {
                    match key.to_ascii_lowercase().as_str() {
                        "vertical" => settings.vertical = Some(value.to_owned()),
                        "line" => settings.line = Some(value.to_owned()),
                        "position" => settings.position = Some(value.to_owned()),
                        "size" => settings.size = Some(value.to_owned()),
                        "align" => settings.align = Some(value.to_owned()),
                        _ => {}
                    }
                }
            }
        }

        let payload_lines = &block[timing_idx + 1..];
        let text = payload_lines.join("\n");

        cues.push(VttCue {
            id,
            start_s,
            end_s,
            settings,
            text,
        });
    }

    Ok(cues)
}

/// Парсит timestamp WebVTT (mm:ss.ttt или hh:mm:ss.ttt). Возвращает секунды.
fn parse_timestamp(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        2 => {
            let (mm, rest) = (parts[0], parts[1]);
            if mm.len() != 2 || !mm.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let mm_val = mm.parse::<u32>().ok()?;
            if mm_val >= 60 {
                return None;
            }
            let (ss, ttt) = rest.split_once('.')?;
            if ss.len() != 2 || !ss.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let ss_val = ss.parse::<u32>().ok()?;
            if ss_val >= 60 {
                return None;
            }
            if ttt.len() != 3 || !ttt.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let ttt_val = ttt.parse::<u32>().ok()?;
            Some(mm_val as f64 * 60.0 + ss_val as f64 + ttt_val as f64 / 1000.0)
        }
        3 => {
            let (hh, mm, rest) = (parts[0], parts[1], parts[2]);
            if hh.is_empty() || !hh.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let hh_val = hh.parse::<u32>().ok()?;
            if mm.len() != 2 || !mm.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let mm_val = mm.parse::<u32>().ok()?;
            if mm_val >= 60 {
                return None;
            }
            let (ss, ttt) = rest.split_once('.')?;
            if ss.len() != 2 || !ss.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let ss_val = ss.parse::<u32>().ok()?;
            if ss_val >= 60 {
                return None;
            }
            if ttt.len() != 3 || !ttt.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let ttt_val = ttt.parse::<u32>().ok()?;
            Some(hh_val as f64 * 3600.0 + mm_val as f64 * 60.0 + ss_val as f64 + ttt_val as f64 / 1000.0)
        }
        _ => None,
    }
}

/// Информация о track-е медиа.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    /// Атрибут kind; по умолчанию "subtitles".
    pub kind: String,
    pub src: String,
    pub srclang: String,
    pub label: String,
    /// Наличие атрибута default.
    pub default: bool,
}

/// Сбор track-ов для всех элементов <video>.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoTracks {
    /// NodeId элемента <video>.
    pub video: NodeId,
    pub tracks: Vec<TrackInfo>,
}

/// Рекурсивно обходит документ и собирает <video> с их <track>.
pub fn collect_video_tracks(doc: &Document) -> Vec<VideoTracks> {
    fn walk(doc: &Document, id: NodeId, out: &mut Vec<VideoTracks>) {
        let node = doc.get(id);
        if node
            .element_name()
            .is_some_and(|n| n.local.eq_ignore_ascii_case("video"))
        {
            let mut tracks = Vec::new();
            for &child in &node.children.clone() {
                let child_node = doc.get(child);
                if child_node
                    .element_name()
                    .is_some_and(|n| n.local.eq_ignore_ascii_case("track"))
                {
                    let Some(src) = child_node.get_attr("src").filter(|s| !s.is_empty()).map(str::to_owned) else { continue };
                    let kind = child_node.get_attr("kind").filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or_else(|| "subtitles".to_string());
                    let srclang = child_node.get_attr("srclang").filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or_default();
                    let label = child_node.get_attr("label").filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or_default();
                    let default = child_node.get_attr("default").is_some();
                    tracks.push(TrackInfo { kind, src, srclang, label, default });
                }
            }
            if !tracks.is_empty() {
                out.push(VideoTracks { video: id, tracks });
            }
        }
        for &child in &node.children.clone() {
            walk(doc, child, out);
        }
    }

    let mut out = Vec::new();
    walk(doc, doc.root(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attribute, NodeData, QualName};

    #[test]
    fn parse_timestamp_basic() {
        assert_eq!(parse_timestamp("00:01.000"), Some(1.0));
        assert_eq!(parse_timestamp("01:02:03.500"), Some(3723.5));
        assert_eq!(parse_timestamp("00:00.000"), Some(0.0));
    }

    #[test]
    fn parse_timestamp_invalid() {
        assert_eq!(parse_timestamp("1:02.000"), None);
        assert_eq!(parse_timestamp("00:61.000"), None);
        assert_eq!(parse_timestamp("00:01.5"), None);
        assert_eq!(parse_timestamp("abc"), None);
    }

    #[test]
    fn parse_vtt_basic() {
        let input = "WEBVTT\n\n00:00.000 --> 00:01.000\nHello";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].id, None);
        assert_eq!(cues[0].start_s, 0.0);
        assert_eq!(cues[0].end_s, 1.0);
        assert_eq!(cues[0].text, "Hello");
        assert_eq!(cues[0].settings, VttCueSettings::default());
    }

    #[test]
    fn parse_vtt_bom() {
        let input = "\u{FEFF}WEBVTT\n\n00:00.000 --> 00:01.000\nA";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "A");
    }

    #[test]
    fn parse_vtt_header_with_comment() {
        let input = "WEBVTT - комментарий\n\n00:00.000 --> 00:01.000\nA";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "A");
    }

    #[test]
    fn parse_vtt_missing_header() {
        assert!(parse_vtt("WEBVTTX\n\n00:00.000 --> 00:01.000\nA") == Err(VttError::MissingHeader));
        assert!(parse_vtt("") == Err(VttError::MissingHeader));
    }

    #[test]
    fn parse_vtt_with_id() {
        let input = "WEBVTT\n\nintro\n00:00.000 --> 00:02.000\nHi";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].id, Some("intro".to_string()));
        assert_eq!(cues[0].text, "Hi");
    }

    #[test]
    fn parse_vtt_skip_note() {
        let input = "WEBVTT\n\nNOTE это комментарий\nвторая строка\n\n00:00.000 --> 00:01.000\nA";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "A");
    }

    #[test]
    fn parse_vtt_settings() {
        let input = "WEBVTT\n\n00:00.000 --> 00:01.000 align:center line:90% position:50%\nA";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].settings.align, Some("center".to_string()));
        assert_eq!(cues[0].settings.line, Some("90%".to_string()));
        assert_eq!(cues[0].settings.position, Some("50%".to_string()));
        assert_eq!(cues[0].settings.vertical, None);
        assert_eq!(cues[0].settings.size, None);
    }

    #[test]
    fn parse_vtt_lenient_invalid_block() {
        let input = "WEBVTT\n\nмусор --> ерунда\nX\n\n00:05.000 --> 00:06.000\nB";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "B");
    }

    #[test]
    fn parse_vtt_start_after_end() {
        let input = "WEBVTT\n\n00:02.000 --> 00:01.000\nX";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 0);
    }

    #[test]
    fn parse_vtt_crlf_multiline() {
        let input = "WEBVTT\r\n\r\n00:00.000 --> 00:01.000\r\nA\r\nB";
        let cues = parse_vtt(input).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "A\nB");
    }

    #[test]
    fn collect_video_tracks_two_tracks() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));

        let track1 = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track1).data {
            attrs.push(Attribute { name: QualName::html("src"), value: "subs_ru.vtt".to_string() });
            attrs.push(Attribute { name: QualName::html("kind"), value: "subtitles".to_string() });
            attrs.push(Attribute { name: QualName::html("srclang"), value: "ru".to_string() });
            attrs.push(Attribute { name: QualName::html("label"), value: "Русские".to_string() });
            attrs.push(Attribute { name: QualName::html("default"), value: String::new() });
        }
        doc.append_child(video, track1);

        let track2 = doc.create_element(QualName::html("track"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track2).data {
            attrs.push(Attribute { name: QualName::html("src"), value: "subs_en.vtt".to_string() });
        }
        doc.append_child(video, track2);

        doc.append_child(doc.root(), video);

        let result = collect_video_tracks(&doc);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tracks.len(), 2);
        assert_eq!(result[0].tracks[0].kind, "subtitles");
        assert_eq!(result[0].tracks[0].src, "subs_ru.vtt");
        assert_eq!(result[0].tracks[0].srclang, "ru");
        assert_eq!(result[0].tracks[0].label, "Русские");
        assert!(result[0].tracks[0].default);
        assert_eq!(result[0].tracks[1].kind, "subtitles");
        assert_eq!(result[0].tracks[1].src, "subs_en.vtt");
        assert_eq!(result[0].tracks[1].srclang, "");
        assert_eq!(result[0].tracks[1].label, "");
        assert!(!result[0].tracks[1].default);
    }

    #[test]
    fn collect_video_tracks_empty_src() {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        let track = doc.create_element(QualName::html("track"));
        // no src attribute
        doc.append_child(video, track);
        doc.append_child(doc.root(), video);
        assert!(collect_video_tracks(&doc).is_empty());
    }

    #[test]
    fn collect_video_tracks_no_video() {
        let doc = Document::new();
        assert!(collect_video_tracks(&doc).is_empty());
    }
}
