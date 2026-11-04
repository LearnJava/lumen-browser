//! Shared WebVTT text-track data for the `TextTrack` JS API (P3-webvtt slice 4).
//!
//! The shell parses `<track>` files into cues (see `shell::tracks`) and mirrors
//! the per-`<video>` result into this process-global store.  The JS native
//! binding `__lumen_texttracks_json(nid)` reads it and hands the page a plain
//! JSON snapshot from which the video shim builds `TextTrackList` / `TextTrack`
//! / `TextTrackCue` objects.
//!
//! # Threading
//!
//! Like [`crate::video_gif_store`], QuickJS and the shell render loop run on the
//! same OS thread, so the inner `Mutex` never blocks; it exists purely for
//! `Sync`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use lumen_dom::vtt::VttCueSettings;
use lumen_dom::vtt_geometry::{CueAlign, CueLine, CueSettings, LineAlign, PositionAlign, Vertical};

// ── Data model ─────────────────────────────────────────────────────────────────

/// One WebVTT cue exposed to JS as a `TextTrackCue` / `VTTCue`.
#[derive(Debug, Clone)]
pub struct CueData {
    /// Cue identifier (`VTTCue.id`); empty when the cue had no id line.
    pub id: String,
    /// Start time in seconds (`TextTrackCue.startTime`).
    pub start: f64,
    /// End time in seconds (`TextTrackCue.endTime`).
    pub end: f64,
    /// Raw cue payload (`VTTCue.text`), tags preserved per spec.
    pub text: String,
    /// Raw cue settings (`VttCueSettings`); serialized to IDL-shaped JSON by
    /// [`cue_settings_json`] so the shim can populate `cue.line` & co (BUG-902
    /// п.2, VTT-RENDER-S2).
    pub settings: VttCueSettings,
}

/// Настраиваемый JSON-объект `settings` одного cue с IDL-значениями
/// (WebVTT §3.1): `line` — число или `"auto"`, `position` — число или
/// `"auto"`, `size` — число, `align`/`lineAlign`/`positionAlign`/`vertical` —
/// строки, `snapToLines` — флаг, `region` — строка или `null`. Сырые строки
/// разбирает [`CueSettings::parse`]; невалидные значения дают значения
/// по умолчанию (BUG-902 п.2).
pub(crate) fn cue_settings_json(raw: &VttCueSettings) -> serde_json::Value {
    let s = CueSettings::parse(raw);
    let vertical = match s.vertical {
        Vertical::Horizontal => "",
        Vertical::Rl => "rl",
        Vertical::Lr => "lr",
    };
    let line = match s.line {
        CueLine::Auto => serde_json::Value::from("auto"),
        CueLine::Number(n) => serde_json::Value::from(n),
        CueLine::Percent(p) => serde_json::Value::from(p),
    };
    let position = match s.position {
        Some(p) => serde_json::Value::from(p),
        None => serde_json::Value::from("auto"),
    };
    let line_align = match s.line_align {
        LineAlign::Start => "start",
        LineAlign::Center => "center",
        LineAlign::End => "end",
    };
    let position_align = match s.position_align {
        PositionAlign::Auto => "auto",
        PositionAlign::LineLeft => "line-left",
        PositionAlign::Center => "center",
        PositionAlign::LineRight => "line-right",
    };
    let align = match s.align {
        CueAlign::Start => "start",
        CueAlign::Center => "center",
        CueAlign::End => "end",
        CueAlign::Left => "left",
        CueAlign::Right => "right",
    };
    serde_json::json!({
        "vertical": vertical,
        "snapToLines": s.snap_to_lines,
        "line": line,
        "lineAlign": line_align,
        "position": position,
        "positionAlign": position_align,
        "size": s.size,
        "align": align,
        "region": s.region,
    })
}

/// One `<track>` element exposed to JS as a `TextTrack`.
#[derive(Debug, Clone)]
pub struct TextTrackData {
    /// `TextTrack.kind` (e.g. `"subtitles"`, `"captions"`, `"chapters"`).
    pub kind: String,
    /// `TextTrack.label` (may be empty).
    pub label: String,
    /// `TextTrack.language` (from the `srclang` attribute; may be empty).
    pub language: String,
    /// `TextTrack.mode`: `"showing"` for the track the shell renders,
    /// `"disabled"` otherwise.
    pub mode: String,
    /// Parsed cues.  Populated only for the showing track; empty otherwise.
    pub cues: Vec<CueData>,
}

// ── Store ──────────────────────────────────────────────────────────────────────

/// Per-`<video>` text-track snapshot, keyed by DOM node index (`el.__nid__`).
///
/// Written by the shell after `<track>` files are fetched and parsed; read by
/// the JS native binding.  Cleared on navigation together with the video store.
#[derive(Default)]
pub struct TextTrackStore {
    /// Key = DOM node index of the `<video>`; value = its ordered tracks.
    pub tracks: Mutex<HashMap<u32, Vec<TextTrackData>>>,
}

impl TextTrackStore {
    /// Serialize the tracks of one `<video>` to a JSON array string.
    ///
    /// Shape: `[{kind,label,language,mode,cues:[{id,start,end,text,settings}]}]`
    /// — `settings` carries the cue's IDL-shaped positioning settings
    /// ([`cue_settings_json`], BUG-902 п.2).
    /// Returns `"[]"` when the video has no tracks.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn tracks_json(&self, nid: u32) -> String {
        let guard = self.tracks.lock().unwrap();
        let Some(tracks) = guard.get(&nid) else {
            return "[]".to_string();
        };
        let arr: Vec<serde_json::Value> = tracks
            .iter()
            .map(|t| {
                let cues: Vec<serde_json::Value> = t
                    .cues
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "id": c.id,
                            "start": c.start,
                            "end": c.end,
                            "text": c.text,
                            "settings": cue_settings_json(&c.settings),
                        })
                    })
                    .collect();
                serde_json::json!({
                    "kind": t.kind,
                    "label": t.label,
                    "language": t.language,
                    "mode": t.mode,
                    "cues": cues,
                })
            })
            .collect();
        serde_json::Value::Array(arr).to_string()
    }
}

// ── Global registry ─────────────────────────────────────────────────────────────

static STORE: OnceLock<RwLock<Option<Arc<TextTrackStore>>>> = OnceLock::new();

fn store_lock() -> &'static RwLock<Option<Arc<TextTrackStore>>> {
    STORE.get_or_init(|| RwLock::new(None))
}

/// Install the text-track store from the shell.
///
/// Should be called once before any JS context is created.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn set_text_track_store(s: Arc<TextTrackStore>) {
    *store_lock().write().unwrap() = Some(s);
}

/// Return a clone of the installed store, or `None` in headless/CI mode.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn get_text_track_store() -> Option<Arc<TextTrackStore>> {
    store_lock().read().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_video_serializes_to_empty_array() {
        let store = TextTrackStore::default();
        assert_eq!(store.tracks_json(7), "[]");
    }

    #[test]
    fn tracks_json_contains_cues() {
        let store = TextTrackStore::default();
        store.tracks.lock().unwrap().insert(
            42,
            vec![TextTrackData {
                kind: "subtitles".to_string(),
                label: "English".to_string(),
                language: "en".to_string(),
                mode: "showing".to_string(),
                cues: vec![CueData {
                    id: "c1".to_string(),
                    start: 0.0,
                    end: 5.0,
                    text: "Hello".to_string(),
                    settings: VttCueSettings::default(),
                }],
            }],
        );
        let json = store.tracks_json(42);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["kind"], "subtitles");
        assert_eq!(v[0]["mode"], "showing");
        assert_eq!(v[0]["cues"][0]["text"], "Hello");
        assert_eq!(v[0]["cues"][0]["start"], 0.0);
        assert_eq!(v[0]["cues"][0]["end"], 5.0);
    }

    #[test]
    fn text_escaped_in_json() {
        let store = TextTrackStore::default();
        store.tracks.lock().unwrap().insert(
            1,
            vec![TextTrackData {
                kind: "captions".to_string(),
                label: String::new(),
                language: String::new(),
                mode: "showing".to_string(),
                cues: vec![CueData {
                    id: String::new(),
                    start: 1.0,
                    end: 2.0,
                    text: "line\"with\"quotes\nand newline".to_string(),
                    settings: VttCueSettings::default(),
                }],
            }],
        );
        let json = store.tracks_json(1);
        // Round-trips through a real JSON parser without corruption.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["cues"][0]["text"], "line\"with\"quotes\nand newline");
    }

    /// VTT-RENDER-S2 / BUG-902 п.2: настройки cue доезжают до JSON-среза
    /// в IDL-форме — `line` процентом даёт число и `snapToLines: false`.
    #[test]
    fn tracks_json_cue_settings_are_idl_shaped() {
        let store = TextTrackStore::default();
        let cues = lumen_dom::vtt::parse_vtt(
            "WEBVTT\n\n00:00.000 --> 00:01.000 line:50% size:50% align:end region:fred\nX",
        )
        .unwrap();
        assert_eq!(cues.len(), 1);
        store.tracks.lock().unwrap().insert(
            3,
            vec![TextTrackData {
                kind: "subtitles".to_string(),
                label: String::new(),
                language: String::new(),
                mode: "showing".to_string(),
                cues: vec![CueData {
                    id: String::new(),
                    start: 0.0,
                    end: 1.0,
                    text: "X".to_string(),
                    settings: cues[0].settings.clone(),
                }],
            }],
        );
        let v: serde_json::Value = serde_json::from_str(&store.tracks_json(3)).unwrap();
        let st = &v[0]["cues"][0]["settings"];
        assert_eq!(st["line"], 50.0);
        assert_eq!(st["snapToLines"], false);
        assert_eq!(st["size"], 50.0);
        assert_eq!(st["align"], "end");
        assert_eq!(st["region"], "fred");
        assert_eq!(st["position"], "auto");
        assert_eq!(st["vertical"], "");
    }

    /// Без настроек (и при невалидных строках) JSON-объект `settings`
    /// несёт значения по умолчанию WebVTT §3.1.
    #[test]
    fn cue_settings_json_defaults() {
        let v = cue_settings_json(&VttCueSettings::default());
        assert_eq!(v["line"], "auto");
        assert_eq!(v["position"], "auto");
        assert_eq!(v["size"], 100.0);
        assert_eq!(v["align"], "center");
        assert_eq!(v["snapToLines"], true);
        assert_eq!(v["lineAlign"], "start");
        assert_eq!(v["positionAlign"], "auto");
        assert_eq!(v["vertical"], "");
        assert_eq!(v["region"], serde_json::Value::Null);
    }
}
