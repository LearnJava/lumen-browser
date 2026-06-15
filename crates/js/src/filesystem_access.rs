//! File System Access API (W3C File System Access §5).
//!
//! Phase 0: `showOpenFilePicker()`, `showSaveFilePicker()`, `showDirectoryPicker()`
//! open native file/directory dialogs and return stub handle objects.

use rquickjs::{Ctx, Exception, Function};
use std::collections::HashMap;
use std::sync::Mutex;

/// File handle registry: id → path.
struct FileHandleRegistry {
    next_id: u32,
    handles: HashMap<u32, String>,
}

impl FileHandleRegistry {
    fn new() -> Self {
        Self {
            next_id: 1,
            handles: HashMap::new(),
        }
    }

    fn allocate(&mut self, path: String) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.handles.insert(id, path);
        id
    }
}

thread_local! {
    static HANDLES: Mutex<FileHandleRegistry> = Mutex::new(FileHandleRegistry::new());
}

/// Native file picker (Windows PowerShell).
#[cfg(target_os = "windows")]
fn show_file_picker() -> Option<String> {
    use std::process::Command;

    let ps_script = r#"
$picker = New-Object Windows.Storage.Pickers.FileOpenPicker
$picker.FileTypeFilter.Add("*")
$file = $picker.PickSingleFileAsync().GetAwaiter().GetResult()
if ($file) { $file.Path }
"#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_script])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native file picker (Linux zenity).
#[cfg(target_os = "linux")]
fn show_file_picker() -> Option<String> {
    use std::process::Command;

    let output = Command::new("zenity")
        .args(&["--file-selection", "--title=Open File"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native file picker (macOS).
#[cfg(target_os = "macos")]
fn show_file_picker() -> Option<String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .args(&["-e", "choose file without invisibles"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native save picker (Windows PowerShell).
#[cfg(target_os = "windows")]
fn show_save_picker(suggested_name: &str) -> Option<String> {
    use std::process::Command;

    let ps_script = format!(
        r#"
$picker = New-Object Windows.Storage.Pickers.FileSavePicker
$picker.SuggestedFileName = "{}"
$picker.FileTypeChoices.Add("All", @("*"))
$file = $picker.PickSaveFileAsync().GetAwaiter().GetResult()
if ($file) {{ $file.Path }}
"#,
        suggested_name.replace('"', "\\\"")
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_script.as_str()])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native save picker (Linux zenity).
#[cfg(target_os = "linux")]
fn show_save_picker(suggested_name: &str) -> Option<String> {
    use std::process::Command;

    let output = Command::new("zenity")
        .args(&[
            "--file-selection",
            "--save",
            &format!("--filename={}", suggested_name),
            "--title=Save File",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native save picker (macOS).
#[cfg(target_os = "macos")]
fn show_save_picker(_suggested_name: &str) -> Option<String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .args(&["-e", "choose file name"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native directory picker (Windows PowerShell).
#[cfg(target_os = "windows")]
fn show_directory_picker() -> Option<String> {
    use std::process::Command;

    let ps_script = r#"
$picker = New-Object Windows.Storage.Pickers.FolderPicker
$folder = $picker.PickSingleFolderAsync().GetAwaiter().GetResult()
if ($folder) { $folder.Path }
"#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_script])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native directory picker (Linux zenity).
#[cfg(target_os = "linux")]
fn show_directory_picker() -> Option<String> {
    use std::process::Command;

    let output = Command::new("zenity")
        .args(&["--file-selection", "--directory", "--title=Choose Folder"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

/// Native directory picker (macOS).
#[cfg(target_os = "macos")]
fn show_directory_picker() -> Option<String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .args(&["-e", "choose folder"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(path.trim().to_string())
    } else {
        None
    }
}

pub(crate) fn install_filesystem_access(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // showOpenFilePicker() → Promise<FileSystemFileHandle[]>
    // Phase 0: simplified, returns array with single stub handle.
    globals.set(
        "showOpenFilePicker",
        Function::new(ctx.clone(), |ctx: Ctx, _: rquickjs::Array| -> rquickjs::Result<String> {
            let path = show_file_picker()
                .ok_or_else(|| Exception::throw_message(&ctx, "AbortError: User cancelled the picker"))?;

            let handle_id = HANDLES.with(|h| h.lock().unwrap().allocate(path.clone()));
            let name = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            // Return JSON string that will be parsed into an object in JS shim
            Ok(format!(r#"{{"__id":{},"name":"{}","__path":"{}"}}"#, handle_id, name, path.replace('"', "\\\"")))
        })?,
    )?;

    // showSaveFilePicker() → Promise<FileSystemFileHandle>
    globals.set(
        "showSaveFilePicker",
        Function::new(ctx.clone(), |ctx: Ctx, _: rquickjs::Array| -> rquickjs::Result<String> {
            let path = show_save_picker("file.txt")
                .ok_or_else(|| Exception::throw_message(&ctx, "AbortError: User cancelled the picker"))?;

            let handle_id = HANDLES.with(|h| h.lock().unwrap().allocate(path.clone()));
            let name = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            Ok(format!(r#"{{"__id":{},"name":"{}","__path":"{}"}}"#, handle_id, name, path.replace('"', "\\\"")))
        })?,
    )?;

    // showDirectoryPicker() → Promise<FileSystemDirectoryHandle>
    globals.set(
        "showDirectoryPicker",
        Function::new(ctx.clone(), |ctx: Ctx, _: rquickjs::Array| -> rquickjs::Result<String> {
            let path = show_directory_picker()
                .ok_or_else(|| Exception::throw_message(&ctx, "AbortError: User cancelled the picker"))?;

            let handle_id = HANDLES.with(|h| h.lock().unwrap().allocate(path.clone()));

            Ok(format!(r#"{{"__id":{},"name":"folder","__path":"{}"}}"#, handle_id, path.replace('"', "\\\"")))
        })?,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_allocate() {
        let mut registry = FileHandleRegistry::new();
        let id1 = registry.allocate("foo/bar.txt".to_string());
        let id2 = registry.allocate("hello/world.rs".to_string());
        assert_ne!(id1, id2);
        assert_eq!(registry.handles.get(&id1), Some(&"foo/bar.txt".to_string()));
        assert_eq!(registry.handles.get(&id2), Some(&"hello/world.rs".to_string()));
    }

    #[test]
    fn test_show_file_picker_stub() {
        let _ = show_file_picker();
    }
}
