//! FontRegistry: системные шрифты + @font-face URL-источники в одном провайдере.
//!
//! Объединяет `SystemFontIndex` (OS-шрифты) и in-memory буферы, загруженные
//! из @font-face `src: url(...)`. Рендер обращается к `read_face_bytes` и
//! получает байты без чтения диска для URL-шрифтов.
//!
//! Виртуальные пути имеют вид `@font-face:<family_lower>/<weight>/<style>`;
//! диска по ним нет — это только ключи для `bytes_store`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use lumen_core::{FaceRecord, FontProvider, FontStyle};

use crate::system_fonts::SystemFontIndex;

/// Провайдер шрифтов с поддержкой @font-face: системные шрифты + URL-буферы.
pub struct FontRegistry {
    system: SystemFontIndex,
    /// family_lowercase → Vec<FaceRecord> с виртуальными путями.
    custom: RwLock<HashMap<String, Vec<FaceRecord>>>,
    /// Виртуальный путь → декодированные байты sfnt (TrueType/OTF).
    bytes_store: RwLock<HashMap<PathBuf, Vec<u8>>>,
}

impl FontRegistry {
    pub fn new() -> Self {
        Self {
            system: SystemFontIndex::new(),
            custom: RwLock::new(HashMap::new()),
            bytes_store: RwLock::new(HashMap::new()),
        }
    }

    /// Registry backed by a custom-dir `SystemFontIndex` — for tests and
    /// headless modes that need predictable font resolution without scanning OS dirs.
    pub fn with_dirs(dirs: Vec<std::path::PathBuf>) -> Self {
        Self {
            system: SystemFontIndex::with_dirs(dirs),
            custom: RwLock::new(HashMap::new()),
            bytes_store: RwLock::new(HashMap::new()),
        }
    }

    /// Регистрирует шрифт из байт-буфера (TrueType / sfnt после декодирования
    /// WOFF/WOFF2). Параметры `family`/`weight`/`style` берутся из дескрипторов
    /// @font-face; байты хранятся в памяти и возвращаются через `read_face_bytes`.
    ///
    /// Если для той же (family, weight, style) запись уже есть — она
    /// заменяется: CSS @font-face последнего правила wins (cascade order).
    pub fn register_from_bytes(&self, family: &str, weight: u16, style: FontStyle, bytes: Vec<u8>) {
        let style_str = match style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        };
        let virt_path = PathBuf::from(format!(
            "@font-face:{}/{}/{}",
            family.to_ascii_lowercase(),
            weight,
            style_str,
        ));

        let record = FaceRecord {
            family: family.to_owned(),
            weight,
            style,
            stretch: 100, // @font-face не явно указывает stretch
            path: virt_path.clone(),
        };

        let key = family.to_ascii_lowercase();
        let mut custom = self.custom.write().unwrap();
        let faces = custom.entry(key).or_default();
        // Заменяем уже существующую запись с тем же virtual path.
        if let Some(existing) = faces.iter_mut().find(|f| f.path == virt_path) {
            *existing = record;
        } else {
            faces.push(record);
        }
        drop(custom);

        self.bytes_store.write().unwrap().insert(virt_path, bytes);
    }

    /// Количество зарегистрированных @font-face face-ов. Для тестов.
    pub fn custom_face_count(&self) -> usize {
        self.custom.read().unwrap().values().map(|v| v.len()).sum()
    }

    /// Resolves a `local()` @font-face source by matching the name against the system
    /// font index (CSS Fonts L4 §4.3: case-insensitive family-name match). If a
    /// system face is found, reads it from disk and returns the raw bytes. Returns
    /// `None` if no matching face exists or the file cannot be read.
    ///
    /// `weight` and `style` are the @font-face rule's own descriptors, used to pick
    /// the closest face from the family (CSS §5.2 matching algorithm).
    pub fn resolve_local_bytes(&self, name: &str, weight: u16, style: FontStyle) -> Option<Vec<u8>> {
        let face = self.system.pick_face(name, weight, style)?;
        std::fs::read(&face.path).ok()
    }

    /// Возвращает байты первого загруженного face для данной семьи.
    ///
    /// Используется [`lumen_paint::MultiFontMeasurer`] в shell для построения
    /// per-family измерителей из @font-face URL-источников.
    pub fn face_bytes_for_family(&self, family: &str) -> Option<Vec<u8>> {
        let key = family.to_ascii_lowercase();
        let custom = self.custom.read().unwrap();
        let face = custom.get(&key)?.first()?;
        let path = face.path.clone();
        drop(custom);
        self.bytes_store.read().unwrap().get(&path).cloned()
    }
}

impl Default for FontRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FontProvider for FontRegistry {
    fn lookup_family(&self, family: &str) -> Vec<PathBuf> {
        let mut paths = self.system.lookup_family(family);
        let key = family.to_ascii_lowercase();
        if let Some(faces) = self.custom.read().unwrap().get(&key) {
            paths.extend(faces.iter().map(|f| f.path.clone()));
        }
        paths
    }

    fn list_families(&self) -> Vec<String> {
        let mut families = self.system.list_families();
        for faces in self.custom.read().unwrap().values() {
            families.extend(faces.iter().map(|f| f.family.clone()));
        }
        families.sort();
        families.dedup();
        families
    }

    fn lookup_faces(&self, family: &str) -> Vec<FaceRecord> {
        let mut faces = self.system.lookup_faces(family);
        let key = family.to_ascii_lowercase();
        if let Some(custom_faces) = self.custom.read().unwrap().get(&key) {
            faces.extend_from_slice(custom_faces);
        }
        faces
    }

    /// Возвращает байты для @font-face виртуальных путей; None для системных
    /// шрифтов (рендер тогда читает через `fs::read`).
    fn read_face_bytes(&self, path: &Path) -> Option<Vec<u8>> {
        self.bytes_store.read().unwrap().get(path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_minimal_ttf() -> Vec<u8> {
        // Минимальный валидный sfnt-заголовок (4 таблицы, все нули).
        // Font::parse дойдёт до таблиц и вернёт ошибку, но нам важны только
        // метаданные регистрации — парсинг не нужен здесь.
        let mut v = Vec::new();
        v.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // sfVersion = 1.0
        v.extend_from_slice(&[0x00, 0x01]); // numTables = 1
        v.extend_from_slice(&[0x00, 0x10, 0x00, 0x01, 0x00, 0x00]); // searchRange, entrySelector, rangeShift
        v
    }

    #[test]
    fn register_and_lookup() {
        let reg = FontRegistry::new();
        reg.register_from_bytes("TestFont", 400, FontStyle::Normal, make_minimal_ttf());
        assert_eq!(reg.custom_face_count(), 1);

        let faces = reg.lookup_faces("TestFont");
        assert!(faces.iter().any(|f| f.family == "TestFont" && f.weight == 400));
    }

    #[test]
    fn read_face_bytes_returns_registered_bytes() {
        let reg = FontRegistry::new();
        let bytes = vec![1u8, 2, 3, 4];
        reg.register_from_bytes("Foo", 700, FontStyle::Italic, bytes.clone());

        let faces = reg.lookup_faces("Foo");
        let face = faces.iter().find(|f| f.weight == 700).unwrap();
        assert_eq!(reg.read_face_bytes(&face.path).unwrap(), bytes);
    }

    #[test]
    fn read_face_bytes_returns_none_for_unknown_path() {
        let reg = FontRegistry::new();
        assert!(reg.read_face_bytes(Path::new("/no/such/font.ttf")).is_none());
    }

    #[test]
    fn replace_existing_entry() {
        let reg = FontRegistry::new();
        reg.register_from_bytes("Bar", 400, FontStyle::Normal, vec![1, 2]);
        reg.register_from_bytes("Bar", 400, FontStyle::Normal, vec![3, 4]);
        // Вторая регистрация заменила первую.
        assert_eq!(reg.custom_face_count(), 1);
        let faces = reg.lookup_faces("Bar");
        let virt = faces.iter().find(|f| f.weight == 400).unwrap().path.clone();
        assert_eq!(reg.read_face_bytes(&virt).unwrap(), vec![3, 4]);
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let reg = FontRegistry::new();
        reg.register_from_bytes("MyFont", 400, FontStyle::Normal, make_minimal_ttf());
        assert!(!reg.lookup_faces("myfont").is_empty());
        assert!(!reg.lookup_faces("MYFONT").is_empty());
    }

    #[test]
    fn list_families_includes_custom() {
        let reg = FontRegistry::new();
        reg.register_from_bytes("CustomSerif", 400, FontStyle::Normal, make_minimal_ttf());
        let families = reg.list_families();
        assert!(families.iter().any(|f| f == "CustomSerif"));
    }

    fn assets_dir() -> std::path::PathBuf {
        // CARGO_MANIFEST_DIR = crates/engine/font → 3 levels up = repo/worktree root
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..").join("..").join("..")
            .join("assets").join("fonts")
    }

    #[test]
    fn resolve_local_finds_bundled_inter() {
        let reg = FontRegistry::with_dirs(vec![assets_dir()]);
        let bytes = reg.resolve_local_bytes("Inter", 400, FontStyle::Normal);
        assert!(bytes.is_some(), "Inter must be found in assets/fonts");
        let b = bytes.unwrap();
        assert!(!b.is_empty());
        // Bytes should parse as a valid font.
        assert!(crate::Font::parse(&b).is_ok());
    }

    #[test]
    fn resolve_local_unknown_family_returns_none() {
        let reg = FontRegistry::with_dirs(vec![assets_dir()]);
        assert!(reg.resolve_local_bytes("NoSuchFontXYZ", 400, FontStyle::Normal).is_none());
    }

    #[test]
    fn resolve_local_case_insensitive() {
        let reg = FontRegistry::with_dirs(vec![assets_dir()]);
        assert!(reg.resolve_local_bytes("inter", 400, FontStyle::Normal).is_some());
        assert!(reg.resolve_local_bytes("INTER", 400, FontStyle::Normal).is_some());
    }

    #[test]
    fn resolve_local_empty_dir_returns_none() {
        let reg = FontRegistry::with_dirs(vec![std::path::PathBuf::from("/no/such/dir")]);
        assert!(reg.resolve_local_bytes("Inter", 400, FontStyle::Normal).is_none());
    }
}
