//! FontRegistry: системные шрифты + @font-face URL-источники в одном провайдере.
//!
//! Объединяет `SystemFontIndex` (OS-шрифты) и in-memory буферы, загруженные
//! из @font-face `src: url(...)`. Рендер обращается к `read_face_bytes` и
//! получает байты без чтения диска для URL-шрифтов.
//!
//! Виртуальные пути имеют вид
//! `@font-face:<family_lower>/<weight>/<style>/<unicode-range-key>`;
//! диска по ним нет — это только ключи для `bytes_store`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use lumen_core::{FaceRecord, FontProvider, FontStyle, NORMAL_STRETCH_PERCENT};

use crate::system_fonts::SystemFontIndex;
use crate::unicode_range::UnicodeRange;

/// Канонический ключ для `unicode_range` — часть идентичности виртуального
/// пути реестра (BUG-434). Пустой список (нет дескриптора `unicode-range`,
/// значит face покрывает все кодпоинты) даёт стабильный ключ `"all"`.
fn unicode_range_key(ranges: &[UnicodeRange]) -> String {
    if ranges.is_empty() {
        return "all".to_string();
    }
    ranges
        .iter()
        .map(|r| format!("{:x}-{:x}", r.start, r.end))
        .collect::<Vec<_>>()
        .join(",")
}

/// Провайдер шрифтов с поддержкой @font-face: системные шрифты + URL-буферы.
pub struct FontRegistry {
    /// Системный индекс. У `new()` — процесс-глобальный
    /// ([`crate::shared_system_index`]), чтобы каждая новая страница не
    /// пересканировала директории шрифтов; у `with_dirs` — собственный.
    system: Arc<SystemFontIndex>,
    /// family_lowercase → Vec<FaceRecord> с виртуальными путями.
    custom: RwLock<HashMap<String, Vec<FaceRecord>>>,
    /// Виртуальный путь → декодированные байты sfnt (TrueType/OTF).
    ///
    /// Хранятся как `Arc<[u8]>` (BUG-272 срез 6), чтобы `read_face_bytes`
    /// отдавал буфер через клон Arc (счётчик ссылок), а не копией всего
    /// файла шрифта — рендер разделяет ту же аллокацию в `LoadedFace`.
    bytes_store: RwLock<HashMap<PathBuf, Arc<[u8]>>>,
}

impl FontRegistry {
    pub fn new() -> Self {
        Self {
            system: Arc::clone(crate::system_fonts::shared_system_index()),
            custom: RwLock::new(HashMap::new()),
            bytes_store: RwLock::new(HashMap::new()),
        }
    }

    /// Registry backed by a custom-dir `SystemFontIndex` — for tests and
    /// headless modes that need predictable font resolution without scanning OS dirs.
    pub fn with_dirs(dirs: Vec<std::path::PathBuf>) -> Self {
        Self {
            system: Arc::new(SystemFontIndex::with_dirs(dirs)),
            custom: RwLock::new(HashMap::new()),
            bytes_store: RwLock::new(HashMap::new()),
        }
    }

    /// Регистрирует шрифт из байт-буфера (TrueType / sfnt после декодирования
    /// WOFF/WOFF2). Параметры `family`/`weight`/`style` берутся из дескрипторов
    /// @font-face; байты хранятся в памяти и возвращаются через `read_face_bytes`.
    ///
    /// Если для той же (family, weight, style, unicode_range) запись уже
    /// есть — она заменяется: CSS @font-face последнего правила wins (cascade
    /// order). Записи с тем же (family, weight, style), но **другим**
    /// `unicode_range`, не конкурируют — CSS Fonts L4 §5.1 трактует их как
    /// сабсеты одной логической семьи, дополняющие друг друга покрытием
    /// кодпоинтов, а не альтернативы. До BUG-434 `unicode_range` не входил в
    /// идентичность записи, поэтому второй и последующие сабсеты одной
    /// (family, weight, style) молча стирали предыдущие — реестр держал
    /// ровно один face вместо всех.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn register_from_bytes(
        &self,
        family: &str,
        weight: u16,
        style: FontStyle,
        unicode_range: &[UnicodeRange],
        bytes: Vec<u8>,
    ) {
        let style_str = match style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        };
        let virt_path = PathBuf::from(format!(
            "@font-face:{}/{}/{}/{}",
            family.to_ascii_lowercase(),
            weight,
            style_str,
            unicode_range_key(unicode_range),
        ));

        let record = FaceRecord {
            family: family.to_owned(),
            weight,
            style,
            // @font-face-дескриптор `font-stretch` сюда пока не доезжает:
            // ключ регистрации — (family, weight, style), и разделить два
            // правила одного семейства по stretch этой схемой нельзя.
            stretch: NORMAL_STRETCH_PERCENT,
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

        self.bytes_store
            .write()
            .unwrap()
            .insert(virt_path, Arc::from(bytes));
    }

    /// Количество зарегистрированных @font-face face-ов. Для тестов.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn custom_face_count(&self) -> usize {
        self.custom.read().unwrap().values().map(|v| v.len()).sum()
    }

    /// Resolves a `local()` @font-face source by matching the name against the system
    /// font index (CSS Fonts L4 §4.3: case-insensitive family-name match). If a
    /// system face is found, reads it from disk and returns the raw bytes. Returns
    /// `None` if no matching face exists or the file cannot be read.
    ///
    /// `weight`, `style` and `stretch` are the @font-face rule's own descriptors,
    /// used to pick the closest face from the family (CSS §5.2 matching algorithm).
    /// `stretch` is in CSS percent ([`lumen_core::NORMAL_STRETCH_PERCENT`] = normal)
    /// and is matched against each face's `usWidthClass`.
    pub fn resolve_local_bytes(
        &self,
        name: &str,
        weight: u16,
        style: FontStyle,
        stretch: u16,
    ) -> Option<Vec<u8>> {
        let face = self.system.pick_face(name, weight, style, stretch)?;
        std::fs::read(&face.path).ok()
    }

    /// Возвращает байты первого загруженного face для данной семьи.
    ///
    /// Используется [`lumen_paint::MultiFontMeasurer`] в shell для построения
    /// per-family измерителей из @font-face URL-источников.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn face_bytes_for_family(&self, family: &str) -> Option<Vec<u8>> {
        let key = family.to_ascii_lowercase();
        let custom = self.custom.read().unwrap();
        let face = custom.get(&key)?.first()?;
        let path = face.path.clone();
        drop(custom);
        // Shell-facing setup path (once per @font-face family) — the owned
        // `Vec<u8>` API is kept; the persistent double-storage BUG-272 targets
        // is the render path (`read_face_bytes` → `LoadedFace`), which shares
        // the `Arc` instead.
        self.bytes_store
            .read()
            .unwrap()
            .get(&path)
            .map(|b| b.to_vec())
    }
}

impl Default for FontRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FontProvider for FontRegistry {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn lookup_family(&self, family: &str) -> Vec<PathBuf> {
        let mut paths = self.system.lookup_family(family);
        let key = family.to_ascii_lowercase();
        if let Some(faces) = self.custom.read().unwrap().get(&key) {
            paths.extend(faces.iter().map(|f| f.path.clone()));
        }
        paths
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn list_families(&self) -> Vec<String> {
        let mut families = self.system.list_families();
        for faces in self.custom.read().unwrap().values() {
            families.extend(faces.iter().map(|f| f.family.clone()));
        }
        families.sort();
        families.dedup();
        families
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
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
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn read_face_bytes(&self, path: &Path) -> Option<Arc<[u8]>> {
        // `cloned()` on `Arc<[u8]>` bumps the refcount — no font-buffer copy.
        self.bytes_store.read().unwrap().get(path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unicode_range::parse_unicode_ranges;

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
        reg.register_from_bytes("TestFont", 400, FontStyle::Normal, &[], make_minimal_ttf());
        assert_eq!(reg.custom_face_count(), 1);

        let faces = reg.lookup_faces("TestFont");
        assert!(faces.iter().any(|f| f.family == "TestFont" && f.weight == 400));
    }

    #[test]
    fn read_face_bytes_returns_registered_bytes() {
        let reg = FontRegistry::new();
        let bytes = vec![1u8, 2, 3, 4];
        reg.register_from_bytes("Foo", 700, FontStyle::Italic, &[], bytes.clone());

        let faces = reg.lookup_faces("Foo");
        let face = faces.iter().find(|f| f.weight == 700).unwrap();
        assert_eq!(&*reg.read_face_bytes(&face.path).unwrap(), &bytes[..]);
    }

    #[test]
    fn read_face_bytes_shares_allocation_across_calls() {
        // BUG-272 срез 6: два вызова read_face_bytes отдают клоны одного Arc —
        // указывают на одну аллокацию (буфер шрифта не копируется на каждый
        // вызов, счётчик ссылок разделяется с bytes_store и LoadedFace рендера).
        let reg = FontRegistry::new();
        reg.register_from_bytes("Shared", 400, FontStyle::Normal, &[], vec![9, 8, 7, 6]);
        let faces = reg.lookup_faces("Shared");
        let path = faces.iter().find(|f| f.weight == 400).unwrap().path.clone();

        let a = reg.read_face_bytes(&path).unwrap();
        let b = reg.read_face_bytes(&path).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "оба клона должны указывать на одну аллокацию");
    }

    #[test]
    fn read_face_bytes_returns_none_for_unknown_path() {
        let reg = FontRegistry::new();
        assert!(reg.read_face_bytes(Path::new("/no/such/font.ttf")).is_none());
    }

    #[test]
    fn replace_existing_entry() {
        let reg = FontRegistry::new();
        reg.register_from_bytes("Bar", 400, FontStyle::Normal, &[], vec![1, 2]);
        reg.register_from_bytes("Bar", 400, FontStyle::Normal, &[], vec![3, 4]);
        // Вторая регистрация заменила первую.
        assert_eq!(reg.custom_face_count(), 1);
        let faces = reg.lookup_faces("Bar");
        let virt = faces.iter().find(|f| f.weight == 400).unwrap().path.clone();
        assert_eq!(&*reg.read_face_bytes(&virt).unwrap(), &[3, 4][..]);
    }

    #[test]
    fn subsets_with_different_unicode_range_coexist() {
        // BUG-434: two @font-face rules for the same (family, weight, style)
        // but non-overlapping unicode-range are subsets of one logical family
        // (CSS Fonts L4 §5.1) — the second must not clobber the first.
        let reg = FontRegistry::new();
        let latin = parse_unicode_ranges("U+0000-00FF");
        let cyrillic = parse_unicode_ranges("U+0400-04FF");
        reg.register_from_bytes("Roboto", 400, FontStyle::Normal, &latin, vec![1, 2]);
        reg.register_from_bytes("Roboto", 400, FontStyle::Normal, &cyrillic, vec![3, 4]);
        assert_eq!(reg.custom_face_count(), 2, "both subsets must be kept, not just the last one");

        let faces = reg.lookup_faces("Roboto");
        assert_eq!(faces.len(), 2);
        let bytes: std::collections::HashSet<Vec<u8>> = faces
            .iter()
            .map(|f| reg.read_face_bytes(&f.path).unwrap().to_vec())
            .collect();
        assert!(bytes.contains(&vec![1u8, 2]), "latin subset bytes must survive");
        assert!(bytes.contains(&vec![3u8, 4]), "cyrillic subset bytes must survive");
    }

    #[test]
    fn re_registering_same_unicode_range_still_replaces() {
        // Re-fetching the same @font-face rule's subset (e.g. after reload)
        // must still replace-in-place, not accumulate duplicates.
        let reg = FontRegistry::new();
        let ranges = parse_unicode_ranges("U+0000-00FF");
        reg.register_from_bytes("Bar", 400, FontStyle::Normal, &ranges, vec![1, 2]);
        reg.register_from_bytes("Bar", 400, FontStyle::Normal, &ranges, vec![3, 4]);
        assert_eq!(reg.custom_face_count(), 1);
        let faces = reg.lookup_faces("Bar");
        assert_eq!(&*reg.read_face_bytes(&faces[0].path).unwrap(), &[3, 4][..]);
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let reg = FontRegistry::new();
        reg.register_from_bytes("MyFont", 400, FontStyle::Normal, &[], make_minimal_ttf());
        assert!(!reg.lookup_faces("myfont").is_empty());
        assert!(!reg.lookup_faces("MYFONT").is_empty());
    }

    #[test]
    fn list_families_includes_custom() {
        let reg = FontRegistry::new();
        reg.register_from_bytes("CustomSerif", 400, FontStyle::Normal, &[], make_minimal_ttf());
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
        let bytes = reg.resolve_local_bytes("Inter", 400, FontStyle::Normal, NORMAL_STRETCH_PERCENT);
        assert!(bytes.is_some(), "Inter must be found in assets/fonts");
        let b = bytes.unwrap();
        assert!(!b.is_empty());
        // Bytes should parse as a valid font.
        assert!(crate::Font::parse(&b).is_ok());
    }

    #[test]
    fn resolve_local_unknown_family_returns_none() {
        let reg = FontRegistry::with_dirs(vec![assets_dir()]);
        assert!(reg.resolve_local_bytes("NoSuchFontXYZ", 400, FontStyle::Normal, NORMAL_STRETCH_PERCENT).is_none());
    }

    #[test]
    fn resolve_local_case_insensitive() {
        let reg = FontRegistry::with_dirs(vec![assets_dir()]);
        assert!(reg.resolve_local_bytes("inter", 400, FontStyle::Normal, NORMAL_STRETCH_PERCENT).is_some());
        assert!(reg.resolve_local_bytes("INTER", 400, FontStyle::Normal, NORMAL_STRETCH_PERCENT).is_some());
    }

    #[test]
    fn resolve_local_empty_dir_returns_none() {
        let reg = FontRegistry::with_dirs(vec![std::path::PathBuf::from("/no/such/dir")]);
        assert!(reg.resolve_local_bytes("Inter", 400, FontStyle::Normal, NORMAL_STRETCH_PERCENT).is_none());
    }
}
