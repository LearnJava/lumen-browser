//! On-disk cache for the system font index (PERF-11).
//!
//! Parsing every system font file's `name`/`OS/2` tables costs ~124 ms on a
//! typical Windows install (174 families, measured in
//! `system_fonts::perf_census::census_system_font_index_build_cost`) — paid
//! on the FIRST layout of every process, even a page with no text at all
//! (`docs/tasks/perf-startup-census.md` §2.3/§4). This module serializes the
//! built index to `<exe_dir>/data/fonts/index.cache` so only the first ever
//! scan (per machine, per set of font-directory contents) pays the full
//! parse cost; every run after reads back a small text file instead of
//! re-parsing hundreds of font files.
//!
//! **Invalidation**: keyed on the mtime + size of each scanned directory
//! (the top-level `dirs` list `system_fonts` hands us, not a recursive walk
//! of every subdirectory — that would cost close to as much as the scan it's
//! trying to avoid) plus the crate's own version, so a font install/removal
//! or a Lumen upgrade both force a fresh scan.
//!
//! **Corruption / staleness must degrade to a full scan, never to an empty
//! index** — a page silently losing every font is worse than an occasional
//! slow layout — so every parse failure returns `None`, never a partial
//! `HashMap`.
//!
//! **Cross-process write race**: `graphic_tests/run.py` and WPT spawn many
//! Lumen processes in quick succession, so writers use write-to-temp +
//! `rename` (atomic on both POSIX and Windows `MoveFileEx`) — a reader never
//! observes a half-written cache file.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use lumen_core::{FaceRecord, FontStyle};

/// Portable data root duplicate — `lumen-font` sits below `lumen-shell` in
/// the dependency graph and cannot call `shell::adblock::browser_data_dir`
/// (CLAUDE.md Known gotchas, user decision 2026-06-16: browser-folder data,
/// never OS dirs). Same duplication already exists in `lumen-storage::hsts`
/// and `lumen-paint::backend_probe`.
fn browser_data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("data")))
        .unwrap_or_else(|| PathBuf::from("data"))
}

/// `<exe_dir>/data/fonts/index.cache`. `pub(crate)` so `system_fonts`'s
/// tests can clear it directly when comparing a warm read against a forced
/// cold scan.
pub(crate) fn cache_path() -> PathBuf {
    browser_data_dir().join("fonts").join("index.cache")
}

/// mtime (unix seconds) + size of one directory — cheap invalidation signal.
/// Not a content hash: a font install/removal changes a directory's own
/// mtime on every platform this project targets, which is enough without
/// re-reading file contents. A missing/unreadable directory signs as
/// `(0, 0)`, same as any other directory that happens to match — harmless,
/// since a real 0-mtime/0-size directory is not a case that occurs on a
/// system actually shipping fonts.
fn dir_signature(dir: &Path) -> (u64, u64) {
    let Ok(meta) = fs::metadata(dir) else {
        return (0, 0);
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (mtime, meta.len())
}

/// A value that would break the line-based cache format if written verbatim.
/// Real font families/paths never contain these; when one somehow does, the
/// safe choice is to skip caching entirely rather than write a file the
/// parser can't read back correctly.
fn is_cache_safe(s: &str) -> bool {
    !s.contains(['\t', '\n', '\r'])
}

fn style_to_str(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
        FontStyle::Oblique => "oblique",
    }
}

fn style_from_str(s: &str) -> Option<FontStyle> {
    match s {
        "normal" => Some(FontStyle::Normal),
        "italic" => Some(FontStyle::Italic),
        "oblique" => Some(FontStyle::Oblique),
        _ => None,
    }
}

const CACHE_HEADER: &str = concat!("lumen-font-cache-v1;app=", env!("CARGO_PKG_VERSION"));

/// Reads the cache and rebuilds the index iff every scanned directory's
/// signature still matches `dirs` (same order, same count) and the file was
/// written by this exact app version. Any mismatch, missing file, or parse
/// error is a cache miss (`None`) — never a partial result.
pub(crate) fn read_index_cache(dirs: &[PathBuf]) -> Option<HashMap<String, Vec<FaceRecord>>> {
    let text = fs::read_to_string(cache_path()).ok()?;
    let mut lines = text.lines();

    if lines.next()? != CACHE_HEADER {
        return None;
    }

    let dir_count: usize = lines.next()?.parse().ok()?;
    if dir_count != dirs.len() {
        return None;
    }
    for dir in dirs {
        let (want_mtime, want_len) = dir_signature(dir);
        let line = lines.next()?;
        let mut parts = line.splitn(3, '\t');
        let path = parts.next()?;
        let mtime: u64 = parts.next()?.parse().ok()?;
        let len: u64 = parts.next()?.parse().ok()?;
        if path != dir.to_string_lossy() || mtime != want_mtime || len != want_len {
            return None;
        }
    }

    let face_count: usize = lines.next()?.parse().ok()?;
    let mut index: HashMap<String, Vec<FaceRecord>> = HashMap::new();
    for _ in 0..face_count {
        let line = lines.next()?;
        let mut parts = line.splitn(5, '\t');
        let family = parts.next()?.to_string();
        let weight: u16 = parts.next()?.parse().ok()?;
        let style = style_from_str(parts.next()?)?;
        let stretch: u16 = parts.next()?.parse().ok()?;
        let path = PathBuf::from(parts.next()?);
        let key = family.to_ascii_lowercase();
        index.entry(key).or_default().push(FaceRecord { family, weight, style, stretch, path });
    }

    if lines.next().is_some() {
        return None; // trailing garbage — treat the whole file as suspect
    }
    Some(index)
}

/// Best-effort write: any I/O failure (no write permission in `data/`,
/// read-only disk, unsafe path/family text) is silently ignored — the next
/// run simply pays the full scan again, exactly as it did before this cache
/// existed.
pub(crate) fn write_index_cache(dirs: &[PathBuf], index: &HashMap<String, Vec<FaceRecord>>) {
    let mut out = String::new();
    out.push_str(CACHE_HEADER);
    out.push('\n');
    out.push_str(&dirs.len().to_string());
    out.push('\n');
    for dir in dirs {
        let path = dir.to_string_lossy();
        if !is_cache_safe(&path) {
            return;
        }
        let (mtime, len) = dir_signature(dir);
        out.push_str(&format!("{path}\t{mtime}\t{len}\n"));
    }

    let faces: Vec<&FaceRecord> = index.values().flatten().collect();
    out.push_str(&faces.len().to_string());
    out.push('\n');
    for face in &faces {
        let path = face.path.to_string_lossy();
        if !is_cache_safe(&face.family) || !is_cache_safe(&path) {
            return;
        }
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{path}\n",
            face.family,
            face.weight,
            style_to_str(face.style),
            face.stretch,
        ));
    }

    let final_path = cache_path();
    let Some(dir) = final_path.parent() else {
        return;
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    // Write-to-temp + rename: several Lumen processes can build the index at
    // the same time (graphic_tests, WPT), and a torn read of a half-written
    // file would look like corruption. The PID makes concurrent writers'
    // temp files distinct; a leftover temp file from a crashed process is
    // harmless clutter, same trade-off as elsewhere in this codebase.
    let tmp_path = dir.join(format!("index.cache.tmp.{}", std::process::id()));
    if fs::write(&tmp_path, &out).is_err() {
        return;
    }
    let _ = fs::rename(&tmp_path, &final_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // All three tests below share the single on-disk cache slot
    // (`cache_path()` is one fixed file) and assert on its exact state
    // right after mutating it — genuinely racy under cargo's default
    // parallel test execution, unlike `system_fonts`'s cache tests (which
    // only assert content equality, safe under races because the content
    // for a given `dirs` signature is deterministic). Serialize them.
    static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn sample_index() -> HashMap<String, Vec<FaceRecord>> {
        let mut m = HashMap::new();
        m.insert(
            "inter".to_string(),
            vec![FaceRecord {
                family: "Inter".to_string(),
                weight: 400,
                style: FontStyle::Normal,
                stretch: 100,
                path: PathBuf::from("/fake/Inter-Regular.ttf"),
            }],
        );
        m
    }

    #[test]
    fn cache_roundtrip_matches_input() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // cache_path() резолвится от current_exe() — при `cargo test` это
        // тестовый бинарник, запись изолирована per-run (тот же приём, что
        // backend_probe::cache_roundtrip_writes_and_reads_back).
        let dirs = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
        let _ = fs::remove_file(cache_path());
        assert!(read_index_cache(&dirs).is_none());

        let index = sample_index();
        write_index_cache(&dirs, &index);
        assert_eq!(read_index_cache(&dirs), Some(index));

        let _ = fs::remove_file(cache_path());
    }

    #[test]
    fn cache_miss_on_changed_dir_signature() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dirs = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
        let _ = fs::remove_file(cache_path());
        write_index_cache(&dirs, &sample_index());
        assert!(read_index_cache(&dirs).is_some());

        // Same file, but asked for a different directory list — must miss,
        // not silently return the wrong family set.
        let other_dirs = vec![PathBuf::from("/definitely/does/not/exist/xyz")];
        assert!(read_index_cache(&other_dirs).is_none());

        let _ = fs::remove_file(cache_path());
    }

    #[test]
    fn corrupt_cache_degrades_to_miss_not_panic() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dirs = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
        let path = cache_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not a valid cache file\nrandom garbage").unwrap();

        assert!(read_index_cache(&dirs).is_none());

        let _ = fs::remove_file(&path);
    }
}
