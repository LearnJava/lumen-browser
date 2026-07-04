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
    /// Shape: `[{kind,label,language,mode,cues:[{id,start,end,text}]}]`.
    /// Returns `"[]"` when the video has no tracks.
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
pub fn set_text_track_store(s: Arc<TextTrackStore>) {
    *store_lock().write().unwrap() = Some(s);
}

/// Return a clone of the installed store, or `None` in headless/CI mode.
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
                }],
            }],
        );
        let json = store.tracks_json(1);
        // Round-trips through a real JSON parser without corruption.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["cues"][0]["text"], "line\"with\"quotes\nand newline");
    }
}
