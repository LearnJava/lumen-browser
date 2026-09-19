//! Minimal ZIP reader for self-update archives (UPD-7, `docs/tasks/ph3-self-update.md`).
//!
//! Reads a self-update release archive straight out of an in-memory buffer:
//! central directory parsing + stored/deflate decompression via the
//! already-vendored `flate2` (`docs/tasks/ph3-self-update.md` §3 — "no new
//! heavy dependencies"). Covers exactly what `.github/workflows/release.yml`
//! produces (`7z a` on Windows, `tar`/deflate elsewhere is out of scope here —
//! only the ZIP container is read): methods 0 (stored) and 8 (deflate), a
//! single-disk central directory, no ZIP64 extension.
//!
//! # Extraction safety
//!
//! [`extract`] enforces the destination whitelist from the self-update
//! brief's "user-data safety" §5: an entry whose name is absolute, escapes
//! the destination via `..`, or targets the `data/` subtree is rejected
//! before anything is written — the zip-slip defense the apply step (UPD-8)
//! relies on.
//!
//! # Wiring status
//!
//! [`extract`] is called by `update.rs`'s `apply_staged_update` (UPD-8).
//! Nothing calls that yet either — the UI trigger is UPD-9.
#![allow(dead_code)]

use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::DeflateDecoder;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_DIR_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;
const EOCD_FIXED_LEN: usize = 22;
const CENTRAL_ENTRY_FIXED_LEN: usize = 46;
const LOCAL_HEADER_FIXED_LEN: usize = 30;
/// EOCD's comment length field is 16-bit — an archive comment is at most this long.
const MAX_EOCD_COMMENT_LEN: usize = 0xFFFF;

/// Everything that can go wrong reading or extracting a ZIP archive.
#[derive(Debug, PartialEq, Eq)]
pub enum ZipError {
    /// No End Of Central Directory record found in the buffer.
    NotAZip,
    /// A central directory or local file header was truncated, or a
    /// signature didn't match at the expected offset.
    Malformed,
    /// Multi-disk archives and ZIP64 (>4 GiB / >65535 entries) are out of
    /// scope — release archives are a few tens of MB.
    Unsupported(&'static str),
    /// Compression method other than stored (0) or deflate (8).
    UnsupportedMethod(u16),
    /// Decompressed size didn't match the central directory's record, or
    /// the deflate stream itself was invalid.
    Corrupt,
    /// Entry name is absolute, escapes the destination via `..`, or targets
    /// the `data/` subtree — see the module's "Extraction safety" note.
    UnsafePath(String),
    /// Filesystem error while writing an extracted entry.
    Io(String),
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipError::NotAZip => write!(f, "not a ZIP archive (no End Of Central Directory record)"),
            ZipError::Malformed => write!(f, "malformed ZIP structure"),
            ZipError::Unsupported(what) => write!(f, "unsupported ZIP feature: {what}"),
            ZipError::UnsupportedMethod(m) => write!(f, "unsupported compression method {m}"),
            ZipError::Corrupt => write!(f, "corrupt entry data"),
            ZipError::UnsafePath(name) => write!(f, "unsafe destination path in entry {name:?}"),
            ZipError::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for ZipError {}

/// One file listed in the central directory.
struct CentralEntry {
    name: String,
    method: u16,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
}

/// A parsed ZIP archive, borrowing the original bytes.
pub struct ZipArchive<'a> {
    buf: &'a [u8],
    entries: Vec<CentralEntry>,
}

fn read_u16(buf: &[u8], offset: usize) -> Result<u16, ZipError> {
    let b = buf.get(offset..offset + 2).ok_or(ZipError::Malformed)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(buf: &[u8], offset: usize) -> Result<u32, ZipError> {
    let b = buf.get(offset..offset + 4).ok_or(ZipError::Malformed)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Scans backwards for the EOCD signature. The archive comment (0-65535
/// bytes) follows the fixed part, so the signature isn't necessarily at a
/// fixed distance from the end of the buffer.
fn find_eocd(buf: &[u8]) -> Result<usize, ZipError> {
    if buf.len() < EOCD_FIXED_LEN {
        return Err(ZipError::NotAZip);
    }
    let scan_from = buf.len().saturating_sub(EOCD_FIXED_LEN + MAX_EOCD_COMMENT_LEN);
    let sig = EOCD_SIGNATURE.to_le_bytes();
    let mut i = buf.len() - EOCD_FIXED_LEN;
    loop {
        if buf[i..i + 4] == sig {
            return Ok(i);
        }
        if i == scan_from {
            return Err(ZipError::NotAZip);
        }
        i -= 1;
    }
}

impl<'a> ZipArchive<'a> {
    /// Parses the central directory of a ZIP archive held entirely in `buf`.
    pub fn parse(buf: &'a [u8]) -> Result<Self, ZipError> {
        let eocd = find_eocd(buf)?;
        let disk_number = read_u16(buf, eocd + 4)?;
        let cd_start_disk = read_u16(buf, eocd + 6)?;
        if disk_number != 0 || cd_start_disk != 0 {
            return Err(ZipError::Unsupported("multi-disk archive"));
        }
        let total_entries = read_u16(buf, eocd + 10)? as usize;
        let cd_size = read_u32(buf, eocd + 12)?;
        let cd_offset = read_u32(buf, eocd + 16)? as usize;
        if cd_size == u32::MAX || cd_offset == u32::MAX as usize {
            return Err(ZipError::Unsupported("ZIP64"));
        }

        let mut entries = Vec::with_capacity(total_entries);
        let mut pos = cd_offset;
        for _ in 0..total_entries {
            let header = buf.get(pos..pos + CENTRAL_ENTRY_FIXED_LEN).ok_or(ZipError::Malformed)?;
            if read_u32(header, 0)? != CENTRAL_DIR_SIGNATURE {
                return Err(ZipError::Malformed);
            }
            let method = read_u16(buf, pos + 10)?;
            let compressed_size = read_u32(buf, pos + 20)?;
            let uncompressed_size = read_u32(buf, pos + 24)?;
            let name_len = read_u16(buf, pos + 28)? as usize;
            let extra_len = read_u16(buf, pos + 30)? as usize;
            let comment_len = read_u16(buf, pos + 32)? as usize;
            let local_header_offset = read_u32(buf, pos + 42)?;
            if compressed_size == u32::MAX || uncompressed_size == u32::MAX || local_header_offset == u32::MAX {
                return Err(ZipError::Unsupported("ZIP64"));
            }
            let name_start = pos + CENTRAL_ENTRY_FIXED_LEN;
            let name_bytes = buf.get(name_start..name_start + name_len).ok_or(ZipError::Malformed)?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();

            entries.push(CentralEntry {
                name,
                method,
                compressed_size: compressed_size as u64,
                uncompressed_size: uncompressed_size as u64,
                local_header_offset: local_header_offset as u64,
            });

            pos = name_start + name_len + extra_len + comment_len;
        }

        Ok(ZipArchive { buf, entries })
    }

    /// Entry names as stored in the central directory, in archive order.
    pub fn entry_names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// Reads and decompresses one entry's body.
    fn read_entry(&self, entry: &CentralEntry) -> Result<Vec<u8>, ZipError> {
        let local_off = entry.local_header_offset as usize;
        let header = self
            .buf
            .get(local_off..local_off + LOCAL_HEADER_FIXED_LEN)
            .ok_or(ZipError::Malformed)?;
        if read_u32(header, 0)? != LOCAL_HEADER_SIGNATURE {
            return Err(ZipError::Malformed);
        }
        let name_len = read_u16(header, 26)? as usize;
        let extra_len = read_u16(header, 28)? as usize;
        let data_start = local_off + LOCAL_HEADER_FIXED_LEN + name_len + extra_len;
        let data_end = data_start
            .checked_add(entry.compressed_size as usize)
            .ok_or(ZipError::Malformed)?;
        let compressed = self.buf.get(data_start..data_end).ok_or(ZipError::Malformed)?;

        let data = match entry.method {
            0 => {
                if compressed.len() as u64 != entry.uncompressed_size {
                    return Err(ZipError::Corrupt);
                }
                compressed.to_vec()
            }
            8 => {
                let mut decoder = DeflateDecoder::new(compressed);
                let mut out = Vec::with_capacity(entry.uncompressed_size as usize);
                decoder.read_to_end(&mut out).map_err(|_| ZipError::Corrupt)?;
                if out.len() as u64 != entry.uncompressed_size {
                    return Err(ZipError::Corrupt);
                }
                out
            }
            other => return Err(ZipError::UnsupportedMethod(other)),
        };
        Ok(data)
    }
}

/// Validates one entry's name against the destination whitelist and returns
/// it as a path relative to the extraction root, or `None` for a pure
/// directory entry (trailing `/`, nothing to write).
///
/// Rejects: empty names, embedded NUL, absolute paths (leading `/` or a
/// drive letter), any `..` component, and the `data/` subtree (case-
/// insensitively — the filesystem this lands on may not be case-sensitive).
fn safe_relative_path(name: &str) -> Result<Option<PathBuf>, ZipError> {
    if name.is_empty() || name.contains('\0') {
        return Err(ZipError::UnsafePath(name.to_string()));
    }
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err(ZipError::UnsafePath(name.to_string()));
    }
    let is_directory_entry = normalized.ends_with('/');

    let mut rel = PathBuf::new();
    for component in normalized.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(ZipError::UnsafePath(name.to_string()));
        }
        rel.push(component);
    }

    if rel.as_os_str().is_empty() {
        // No real components at all (e.g. "/" or "."): nothing to validate further.
        return Ok(None);
    }
    if let Some(std::path::Component::Normal(first)) = rel.components().next()
        && first.eq_ignore_ascii_case("data")
    {
        return Err(ZipError::UnsafePath(name.to_string()));
    }
    if is_directory_entry {
        return Ok(None);
    }
    Ok(Some(rel))
}

/// Extracts every file entry of `buf` under `dest_dir`, returning the
/// written paths in archive order. Directory entries are skipped (parent
/// directories are created on demand from file entries' paths). See the
/// module's "Extraction safety" note for the destination whitelist.
pub fn extract(buf: &[u8], dest_dir: &Path) -> Result<Vec<PathBuf>, ZipError> {
    let archive = ZipArchive::parse(buf)?;
    let mut written = Vec::new();
    for entry in &archive.entries {
        let Some(rel) = safe_relative_path(&entry.name)? else {
            continue;
        };
        let data = archive.read_entry(entry)?;
        let dest = dest_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ZipError::Io(e.to_string()))?;
        }
        std::fs::write(&dest, &data).map_err(|e| ZipError::Io(e.to_string()))?;
        written.push(dest);
    }
    Ok(written)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

    /// Hand-assembles a spec-shaped ZIP (local headers + central directory +
    /// EOCD) from `entries` — `(name, body, store)`, `store == true` uses
    /// method 0, `false` runs `body` through a real `DeflateEncoder` (method
    /// 8). This is the only way to build fixtures without a ZIP-writing
    /// dependency (the brief's whole point is not adding one). `pub(crate)`
    /// so `update.rs`'s own apply-step tests (UPD-8) can build a fixture
    /// archive without duplicating this logic.
    pub(crate) fn build_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut central = Vec::new();
        let mut offsets = Vec::new();

        for (name, body, store) in entries {
            offsets.push(buf.len() as u32);
            let (method, compressed): (u16, Vec<u8>) = if *store {
                (0, body.to_vec())
            } else {
                let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
                enc.write_all(body).expect("deflate encode into a Vec never fails");
                (8, enc.finish().expect("deflate finish into a Vec never fails"))
            };

            buf.extend_from_slice(&LOCAL_HEADER_SIGNATURE.to_le_bytes());
            buf.extend_from_slice(&20u16.to_le_bytes()); // version needed
            buf.extend_from_slice(&0u16.to_le_bytes()); // flags
            buf.extend_from_slice(&method.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes()); // mod time
            buf.extend_from_slice(&0u16.to_le_bytes()); // mod date
            buf.extend_from_slice(&0u32.to_le_bytes()); // crc32 (unchecked by our reader)
            buf.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes()); // extra len
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(&compressed);
        }

        for ((name, body, _), &local_offset) in entries.iter().zip(&offsets) {
            let (method, compressed_len): (u16, u32) = {
                // Recompute deterministically instead of storing state — cheap at fixture scale.
                if entries.iter().find(|e| e.0 == *name).map(|e| e.2) == Some(true) {
                    (0, body.len() as u32)
                } else {
                    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
                    enc.write_all(body).expect("deflate encode into a Vec never fails");
                    (8, enc.finish().expect("deflate finish into a Vec never fails").len() as u32)
                }
            };
            central.extend_from_slice(&CENTRAL_DIR_SIGNATURE.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // mod time
            central.extend_from_slice(&0u16.to_le_bytes()); // mod date
            central.extend_from_slice(&0u32.to_le_bytes()); // crc32
            central.extend_from_slice(&compressed_len.to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra len
            central.extend_from_slice(&0u16.to_le_bytes()); // comment len
            central.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            central.extend_from_slice(&local_offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }

        let cd_offset = buf.len() as u32;
        let cd_size = central.len() as u32;
        buf.extend_from_slice(&central);

        buf.extend_from_slice(&EOCD_SIGNATURE.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // disk number
        buf.extend_from_slice(&0u16.to_le_bytes()); // disk with cd start
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&cd_size.to_le_bytes());
        buf.extend_from_slice(&cd_offset.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // comment len

        buf
    }

    #[test]
    fn extracts_stored_and_deflated_entries() {
        let zip = build_zip(&[
            ("lumen.exe", b"fake exe bytes, stored", true),
            ("lumen-network-service.exe", b"fake service bytes, deflated repeated repeated", false),
        ]);
        let dir = std::env::temp_dir().join(format!("lumen-zip-test-{}", std::process::id()));
        let written = extract(&zip, &dir).expect("extraction of a well-formed archive must succeed");
        assert_eq!(written.len(), 2);
        assert_eq!(
            std::fs::read(dir.join("lumen.exe")).expect("extracted file must exist"),
            b"fake exe bytes, stored"
        );
        assert_eq!(
            std::fs::read(dir.join("lumen-network-service.exe")).expect("extracted file must exist"),
            b"fake service bytes, deflated repeated repeated"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_names_lists_archive_order() {
        let zip = build_zip(&[("a.txt", b"a", true), ("b.txt", b"b", true)]);
        let archive = ZipArchive::parse(&zip).expect("archive is well-formed");
        assert_eq!(archive.entry_names().collect::<Vec<_>>(), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn rejects_parent_traversal() {
        let zip = build_zip(&[("../evil.exe", b"x", true)]);
        let dir = std::env::temp_dir().join("lumen-zip-test-traversal");
        assert_eq!(extract(&zip, &dir), Err(ZipError::UnsafePath("../evil.exe".to_string())));
    }

    #[test]
    fn rejects_absolute_path() {
        let zip = build_zip(&[("/etc/passwd", b"x", true)]);
        let dir = std::env::temp_dir().join("lumen-zip-test-abs");
        assert_eq!(extract(&zip, &dir), Err(ZipError::UnsafePath("/etc/passwd".to_string())));
    }

    #[test]
    fn rejects_windows_drive_absolute_path() {
        let zip = build_zip(&[("C:\\Windows\\System32\\evil.exe", b"x", true)]);
        let dir = std::env::temp_dir().join("lumen-zip-test-drive");
        assert_eq!(
            extract(&zip, &dir),
            Err(ZipError::UnsafePath("C:\\Windows\\System32\\evil.exe".to_string()))
        );
    }

    #[test]
    fn rejects_data_subtree() {
        let zip = build_zip(&[("data/profiles/default.db", b"x", true)]);
        let dir = std::env::temp_dir().join("lumen-zip-test-data");
        assert_eq!(
            extract(&zip, &dir),
            Err(ZipError::UnsafePath("data/profiles/default.db".to_string()))
        );
    }

    #[test]
    fn rejects_data_subtree_case_insensitively() {
        let zip = build_zip(&[("DATA/profiles/default.db", b"x", true)]);
        let dir = std::env::temp_dir().join("lumen-zip-test-data-case");
        assert_eq!(
            extract(&zip, &dir),
            Err(ZipError::UnsafePath("DATA/profiles/default.db".to_string()))
        );
    }

    #[test]
    fn directory_entry_is_skipped_not_written() {
        let zip = build_zip(&[("subdir/", b"", true), ("subdir/file.txt", b"hi", true)]);
        let dir = std::env::temp_dir().join(format!("lumen-zip-test-dir-{}", std::process::id()));
        let written = extract(&zip, &dir).expect("archive with a directory entry must still extract");
        assert_eq!(written, vec![dir.join("subdir").join("file.txt")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_truncated_buffer() {
        assert_eq!(ZipArchive::parse(b"not a zip").err(), Some(ZipError::NotAZip));
    }

    #[test]
    fn rejects_unsupported_compression_method() {
        // LZMA (method 14) central directory entry, hand-built since `build_zip`
        // only ever emits 0/8.
        let mut zip = build_zip(&[("f.bin", b"x", true)]);
        // Method field lives at offset 8 of the local header (offset 0) and at
        // the matching central-directory entry; flip both to an unsupported id.
        zip[8] = 14;
        let cd_offset = read_u32(&zip, zip.len() - 22 + 16).unwrap() as usize;
        zip[cd_offset + 10] = 14;
        assert_eq!(extract(&zip, Path::new("/tmp/unused")), Err(ZipError::UnsupportedMethod(14)));
    }

    #[test]
    fn real_world_zip_fixture_parses_and_extracts() {
        // Vendored WPT fixture (tests/wpt/FileAPI/filelist-section/support/upload.zip),
        // a genuine third-party zip writer's output — not one our own `build_zip`
        // produced — exercising the reader against a real deflate stream and
        // real central-directory layout end to end.
        let bytes = include_bytes!("../tests/fixtures/real-sample.zip");
        let archive = ZipArchive::parse(bytes).expect("real-world fixture must parse");
        assert_eq!(archive.entry_names().collect::<Vec<_>>(), vec!["uploadfile2.txt"]);

        let dir = std::env::temp_dir().join(format!("lumen-zip-test-real-{}", std::process::id()));
        let written = extract(bytes, &dir).expect("real-world fixture must extract");
        assert_eq!(written, vec![dir.join("uploadfile2.txt")]);
        let content = std::fs::read(dir.join("uploadfile2.txt")).expect("extracted file must exist");
        assert_eq!(content.len(), 43);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
