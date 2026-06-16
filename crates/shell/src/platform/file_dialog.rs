//! Native OS file-open dialog for `<input type="file">`.
//!
//! Windows: spawns a PowerShell one-liner that opens `System.Windows.Forms.OpenFileDialog`.
//! Other platforms: no-op (returns empty list). Phase 1 — wire GTK/AppKit on Linux/macOS.
//!
//! Called from `main.rs` when `FormClickAction::OpenFilePicker` fires. Blocks the
//! calling thread until the user selects files or cancels.

/// Metadata for one file returned by the OS picker.
// Fields are read only inside the quickjs delivery path; allow dead_code when
// the quickjs feature is disabled so clippy stays clean on all configurations.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "quickjs"), allow(dead_code))]
pub struct FilePickerEntry {
    /// Filename without directory component (e.g. `"photo.jpg"`).
    pub name: String,
    /// Full absolute path (e.g. `"C:\\Users\\user\\photo.jpg"`).
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// MIME type — empty string when not determinable from extension.
    pub mime_type: String,
    /// `lastModified` in milliseconds since Unix epoch.
    pub last_modified_ms: u64,
}

/// Open the OS file-picker dialog and return selected files.
///
/// `accept` — value of the `accept` attribute (e.g. `"image/*,.pdf"`); used as a
/// filter hint. Phase 0: ignored on all platforms.
/// `multiple` — whether the user may select more than one file.
///
/// Returns an empty `Vec` on cancellation or on non-Windows platforms (Phase 0).
pub fn open_file_dialog(_accept: &str, multiple: bool) -> Vec<FilePickerEntry> {
    #[cfg(target_os = "windows")]
    {
        open_file_dialog_windows(multiple)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = multiple;
        Vec::new()
    }
}

/// Build a JSON array that includes opaque `token` values instead of raw paths.
///
/// Phase 1 format: shell registers each path via `lumen_js::file_input::register_file_token`
/// before calling this, and passes the resulting tokens here.  JS receives tokens only —
/// never raw file system paths.
#[cfg(feature = "quickjs")]
pub fn entries_to_json_with_tokens(entries: &[FilePickerEntry], tokens: &[u64]) -> String {
    let items: Vec<String> = entries
        .iter()
        .zip(tokens.iter())
        .map(|(e, &token)| {
            format!(
                r#"{{"name":{name},"token":{token},"size":{size},"mime_type":{mime},"last_modified_ms":{ts}}}"#,
                name = json_str(&e.name),
                token = token,
                size = e.size,
                mime = json_str(&e.mime_type),
                ts = e.last_modified_ms,
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

#[cfg(feature = "quickjs")]
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn open_file_dialog_windows(multiple: bool) -> Vec<FilePickerEntry> {
    use std::process::Command;
    let multi_flag = if multiple { "$true" } else { "$false" };
    // Use .NET Windows Forms dialog via PowerShell. Outputs one path per line.
    let script = format!(
        "[System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; \
         $d = New-Object System.Windows.Forms.OpenFileDialog; \
         $d.Multiselect = {multi_flag}; \
         if ($d.ShowDialog() -eq 'OK') {{ $d.FileNames -join [char]10 }}"
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();
    let Ok(out) = output else { return Vec::new(); };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(entry_from_path)
        .collect()
}

// BUG-168: used only by platform-cfg'd callers; dead on configs that exclude them.
#[allow(dead_code)]
fn entry_from_path(path: &str) -> FilePickerEntry {
    use std::path::Path;
    let p = Path::new(path);
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let meta = std::fs::metadata(p);
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let last_modified_ms = meta
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    FilePickerEntry {
        name,
        path: path.to_string(),
        size,
        mime_type: String::new(),
        last_modified_ms,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[cfg(feature = "quickjs")]
mod tests {
    use super::*;

    #[test]
    fn json_str_escapes_backslash() {
        assert_eq!(json_str("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn json_str_escapes_quote() {
        assert_eq!(json_str("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn entries_to_json_with_tokens_empty() {
        assert_eq!(entries_to_json_with_tokens(&[], &[]), "[]");
    }

    #[test]
    fn entries_to_json_with_tokens_single() {
        let e = FilePickerEntry {
            name: "report.pdf".to_string(),
            path: "C:\\Users\\user\\report.pdf".to_string(),
            size: 4096,
            mime_type: "application/pdf".to_string(),
            last_modified_ms: 9999,
        };
        let json = entries_to_json_with_tokens(&[e], &[42]);
        assert!(json.contains("\"name\":\"report.pdf\""));
        assert!(json.contains("\"token\":42"));
        assert!(json.contains("\"size\":4096"));
        // Path must NOT appear in token-based JSON
        assert!(!json.contains("path"), "raw path must not be in token JSON");
    }

    #[test]
    fn entries_to_json_with_tokens_multiple() {
        let entries = vec![
            FilePickerEntry {
                name: "a.txt".to_string(),
                path: "/tmp/a.txt".to_string(),
                size: 10,
                mime_type: "text/plain".to_string(),
                last_modified_ms: 0,
            },
            FilePickerEntry {
                name: "b.png".to_string(),
                path: "/tmp/b.png".to_string(),
                size: 200,
                mime_type: "image/png".to_string(),
                last_modified_ms: 1000,
            },
        ];
        let json = entries_to_json_with_tokens(&entries, &[7, 8]);
        assert!(json.contains("\"token\":7"));
        assert!(json.contains("\"token\":8"));
        assert!(json.contains("\"name\":\"a.txt\""));
        assert!(json.contains("\"name\":\"b.png\""));
    }
}
