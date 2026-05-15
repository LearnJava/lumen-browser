//! Индекс системных шрифтов: реализация [`lumen_core::FontProvider`].
//!
//! Сканирует стандартные директории шрифтов текущей ОС, парсит таблицы
//! `name` и `OS/2` каждого `.ttf` / `.otf` файла, строит
//! `HashMap<family_lowercase, Vec<FaceRecord>>`. По одному family обычно
//! несколько face-ов (Regular / Bold / Italic / разные weight-ы) — поэтому Vec.
//!
//! Без сторонних зависимостей: только `std::fs::read_dir` и наши `name` /
//! `OS/2` парсеры. На Linux обходим традиционные пути (`/usr/share/fonts`,
//! `~/.local/share/fonts` и т.д.); на Windows — `C:\Windows\Fonts`; на
//! macOS — `/System/Library/Fonts`, `/Library/Fonts`, `~/Library/Fonts`.
//!
//! Индекс строится лениво при первом `lookup_*` / `list_families`,
//! чтобы конструктор оставался дёшевым (`SystemFontIndex::new()` не делает
//! I/O). После первого скана результат кэшируется навсегда: live-watching
//! директорий шрифтов — задача отдельная, в практике браузер всё равно
//! пересоздаётся редко.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use lumen_core::{FaceRecord, FontProvider, FontStyle};

use crate::face::Font;

/// Простой ленивый индекс системных шрифтов.
///
/// Делать его `Clone` нельзя из-за `OnceLock`-а — индекс строится один раз
/// на инстанс. Если нужно делить между потоками — оборачивай в `Arc`.
pub struct SystemFontIndex {
    /// Директории, которые будут просканированы. Можно переопределить через
    /// [`SystemFontIndex::with_dirs`] (тесты, headless-режимы).
    dirs: Vec<PathBuf>,
    /// HashMap<lowercase family, Vec<FaceRecord>>. Lowercase для CSS-style
    /// case-insensitive matching (Fonts L4 §4.3).
    index: OnceLock<HashMap<String, Vec<FaceRecord>>>,
}

impl SystemFontIndex {
    /// Индекс, который при первом lookup просканирует стандартные пути
    /// текущей ОС. Конструктор не делает I/O — это случится при первом
    /// вызове `lookup_*` / `list_families`.
    pub fn new() -> Self {
        Self {
            dirs: default_font_dirs(),
            index: OnceLock::new(),
        }
    }

    /// Индекс с явно заданным списком директорий — для тестов и
    /// специальных конфигураций. Не добавляет дефолтных путей.
    pub fn with_dirs(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            index: OnceLock::new(),
        }
    }

    fn index(&self) -> &HashMap<String, Vec<FaceRecord>> {
        self.index.get_or_init(|| build_index(&self.dirs))
    }

    /// Сколько family-имён зарегистрировано. Для тестов и диагностики;
    /// `list_families` даёт сами имена.
    pub fn family_count(&self) -> usize {
        self.index().len()
    }
}

impl Default for SystemFontIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl FontProvider for SystemFontIndex {
    fn lookup_family(&self, family: &str) -> Vec<PathBuf> {
        let key = family.to_ascii_lowercase();
        self.index()
            .get(&key)
            .map(|faces| faces.iter().map(|f| f.path.clone()).collect())
            .unwrap_or_default()
    }

    fn list_families(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .index()
            .values()
            .flat_map(|faces| faces.iter().map(|f| f.family.clone()))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn lookup_faces(&self, family: &str) -> Vec<FaceRecord> {
        let key = family.to_ascii_lowercase();
        self.index().get(&key).cloned().unwrap_or_default()
    }
}

fn build_index(dirs: &[PathBuf]) -> HashMap<String, Vec<FaceRecord>> {
    let mut index: HashMap<String, Vec<FaceRecord>> = HashMap::new();
    for dir in dirs {
        scan_dir(dir, &mut index);
    }
    index
}

/// Рекурсивный обход директории. Битые файлы / файлы без `name` таблицы
/// тихо пропускаются — у системных шрифтов это норма (битмап-шрифты,
/// .pfb-файлы и прочее).
fn scan_dir(dir: &Path, index: &mut HashMap<String, Vec<FaceRecord>>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ty = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ty.is_dir() {
            scan_dir(&path, index);
            continue;
        }
        if !is_supported_extension(&path) {
            continue;
        }
        if let Some(face) = read_face(&path) {
            index
                .entry(face.family.to_ascii_lowercase())
                .or_default()
                .push(face);
        }
    }
}

fn is_supported_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf")
}

/// Парсит OS/2 + name → FaceRecord. None — файл не похож на шрифт или
/// нет минимально нужных таблиц (`name` без family).
///
/// Если `OS/2` отсутствует или не парсится (старые .ttf / битые файлы) —
/// face всё равно индексируется с догадкой по `subfamily` («Bold Italic»,
/// «Light»…). Если и subfamily пустой — дефолт Regular 400 Normal.
fn read_face(path: &Path) -> Option<FaceRecord> {
    let bytes = std::fs::read(path).ok()?;
    let font = Font::parse(&bytes).ok()?;
    let name = font.name().ok()?;
    let family = name.best_family()?.to_owned();
    let subfamily = name.subfamily.as_deref().unwrap_or("");

    let (weight, style) = match font.os2() {
        Ok(os2) => {
            let style = if os2.is_italic() {
                FontStyle::Italic
            } else if os2.is_oblique() {
                FontStyle::Oblique
            } else {
                FontStyle::Normal
            };
            (os2.weight_class, style)
        }
        Err(_) => guess_from_subfamily(subfamily),
    };

    Some(FaceRecord {
        family,
        weight,
        style,
        path: path.to_owned(),
    })
}

/// Резерв на случай отсутствия OS/2: парсим subfamily-строку из таблицы
/// `name` («Bold Italic», «Light», «ExtraBold», «Thin»). Для современных
/// шрифтов почти всегда есть OS/2 — это путь только для legacy / битых.
///
/// Возвращает `(weight, style)` — числа из стандартной CSS-шкалы
/// (100, 200, …, 900) и три дискретных style-значения.
fn guess_from_subfamily(subfamily: &str) -> (u16, FontStyle) {
    let lower = subfamily.to_ascii_lowercase();
    let style = if lower.contains("italic") {
        FontStyle::Italic
    } else if lower.contains("oblique") {
        FontStyle::Oblique
    } else {
        FontStyle::Normal
    };
    // Порядок проверок важен: «extra bold» проверяется раньше «bold», иначе
    // «extrabold» свалится в weight=700.
    let weight = if lower.contains("thin") || lower.contains("hairline") {
        100
    } else if lower.contains("extralight")
        || lower.contains("extra light")
        || lower.contains("ultralight")
    {
        200
    } else if lower.contains("light") {
        300
    } else if lower.contains("medium") {
        500
    } else if lower.contains("semibold")
        || lower.contains("semi bold")
        || lower.contains("demibold")
        || lower.contains("demi bold")
    {
        600
    } else if lower.contains("extrabold")
        || lower.contains("extra bold")
        || lower.contains("ultrabold")
    {
        800
    } else if lower.contains("black") || lower.contains("heavy") {
        900
    } else if lower.contains("bold") {
        700
    } else {
        400
    };
    (weight, style)
}

/// Стандартные директории шрифтов по платформам. Каждая возвращённая
/// директория может отсутствовать — обработчик `scan_dir` тихо проигнорирует.
fn default_font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            dirs.push(home.join(".fonts"));
            dirs.push(home.join(".local/share/fonts"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(win) = std::env::var_os("WINDIR") {
            dirs.push(PathBuf::from(win).join("Fonts"));
        } else {
            dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        }
        // Per-user шрифты в Windows 10+ лежат тут:
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library").join("Fonts"));
        }
    }

    // Для остальных ОС (BSD-ы, экзотика) — Linux-подобные пути в качестве
    // best effort; если их нет, scan_dir тихо проигнорирует.
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("assets")
            .join("fonts")
    }

    #[test]
    fn empty_index_when_dirs_dont_exist() {
        let idx = SystemFontIndex::with_dirs(vec![PathBuf::from("/definitely/does/not/exist/xyz")]);
        assert_eq!(idx.family_count(), 0);
        assert!(idx.list_families().is_empty());
        assert!(idx.lookup_family("Inter").is_empty());
        assert!(idx.lookup_faces("Inter").is_empty());
        assert!(idx.pick_face("Inter", 400, FontStyle::Normal).is_none());
    }

    #[test]
    fn finds_bundled_inter() {
        let idx = SystemFontIndex::with_dirs(vec![assets_dir()]);
        assert_eq!(idx.family_count(), 1, "should find exactly one family in assets/fonts");
        let paths = idx.lookup_family("Inter");
        assert_eq!(paths.len(), 1, "Inter Regular registered once");
        assert!(paths[0].file_name().unwrap().to_string_lossy().contains("Inter"));
    }

    #[test]
    fn bundled_inter_has_metadata() {
        // Inter-Regular.ttf содержит OS/2: weight=400, fs_selection без italic.
        let idx = SystemFontIndex::with_dirs(vec![assets_dir()]);
        let faces = idx.lookup_faces("Inter");
        assert_eq!(faces.len(), 1);
        let face = &faces[0];
        assert_eq!(face.family, "Inter");
        assert_eq!(face.weight, 400);
        assert_eq!(face.style, FontStyle::Normal);
    }

    #[test]
    fn pick_face_returns_only_face_for_inter() {
        let idx = SystemFontIndex::with_dirs(vec![assets_dir()]);
        let face = idx.pick_face("Inter", 700, FontStyle::Italic).unwrap();
        // У нас нет Bold Italic, так что matcher вернёт единственный имеющийся.
        assert_eq!(face.weight, 400);
        assert_eq!(face.style, FontStyle::Normal);
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let idx = SystemFontIndex::with_dirs(vec![assets_dir()]);
        assert_eq!(idx.lookup_family("inter").len(), 1);
        assert_eq!(idx.lookup_family("INTER").len(), 1);
        assert_eq!(idx.lookup_family("Inter").len(), 1);
        assert_eq!(idx.lookup_faces("inter").len(), 1);
        assert!(idx.pick_face("INTER", 400, FontStyle::Normal).is_some());
    }

    #[test]
    fn unknown_family_returns_empty() {
        let idx = SystemFontIndex::with_dirs(vec![assets_dir()]);
        assert!(idx.lookup_family("NoSuchFont").is_empty());
        assert!(idx.lookup_faces("NoSuchFont").is_empty());
    }

    #[test]
    fn non_font_files_are_ignored() {
        // assets/fonts содержит и Inter-Regular.ttf, и OFL.txt — OFL.txt
        // не должен попасть в индекс.
        let idx = SystemFontIndex::with_dirs(vec![assets_dir()]);
        let families = idx.list_families();
        for f in &families {
            assert!(
                !f.eq_ignore_ascii_case("ofl") && !f.contains(".txt"),
                "non-font file leaked into index: {f}"
            );
        }
    }

    #[test]
    fn explicit_dir_does_not_pull_in_defaults() {
        // Если пользователь явно указал директорию через with_dirs — мы
        // не должны мешать к ней дефолтные пути системы.
        let idx = SystemFontIndex::with_dirs(vec![PathBuf::from("/tmp/no/such")]);
        assert_eq!(idx.family_count(), 0);
    }

    #[test]
    fn guess_from_subfamily_recognises_common_styles() {
        assert_eq!(guess_from_subfamily("Regular"), (400, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("Bold"), (700, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("Italic"), (400, FontStyle::Italic));
        assert_eq!(guess_from_subfamily("Bold Italic"), (700, FontStyle::Italic));
        assert_eq!(guess_from_subfamily("Light"), (300, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("Medium Italic"), (500, FontStyle::Italic));
        assert_eq!(guess_from_subfamily("ExtraBold"), (800, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("Extra Bold"), (800, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("SemiBold"), (600, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("Black"), (900, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("Thin"), (100, FontStyle::Normal));
        assert_eq!(guess_from_subfamily("Oblique"), (400, FontStyle::Oblique));
        assert_eq!(guess_from_subfamily(""), (400, FontStyle::Normal));
    }
}
