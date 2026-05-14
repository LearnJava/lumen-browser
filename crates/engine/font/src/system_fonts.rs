//! Индекс системных шрифтов: реализация [`lumen_core::FontProvider`].
//!
//! Сканирует стандартные директории шрифтов текущей ОС, парсит таблицу
//! `name` каждого `.ttf` / `.otf` файла, строит `HashMap<family_lowercase,
//! Vec<PathBuf>>`. По одному family обычно несколько face-ов (Regular /
//! Bold / Italic / …) — поэтому Vec.
//!
//! Без сторонних зависимостей: только `std::fs::read_dir` и наш `name`
//! парсер. На Linux обходим традиционные пути (`/usr/share/fonts`,
//! `~/.local/share/fonts` и т.д.); на Windows — `C:\Windows\Fonts`; на
//! macOS — `/System/Library/Fonts`, `/Library/Fonts`, `~/Library/Fonts`.
//!
//! Индекс строится лениво при первом `lookup_family` / `list_families`,
//! чтобы конструктор оставался дёшевым (`SystemFontIndex::new()` не делает
//! I/O). После первого скана результат кэшируется навсегда: live-watching
//! директорий шрифтов — задача отдельная, в практике браузер всё равно
//! пересоздаётся редко.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use lumen_core::FontProvider;

use crate::face::Font;

/// Простой ленивый индекс системных шрифтов.
///
/// Делать его `Clone` нельзя из-за `OnceLock`-а — индекс строится один раз
/// на инстанс. Если нужно делить между потоками — оборачивай в `Arc`.
pub struct SystemFontIndex {
    /// Директории, которые будут просканированы. Можно переопределить через
    /// [`SystemFontIndex::with_dirs`] (тесты, headless-режимы).
    dirs: Vec<PathBuf>,
    /// HashMap<lowercase family, Vec<PathBuf>>. Lowercase для CSS-style
    /// case-insensitive matching (Fonts L4 §4.3).
    index: OnceLock<HashMap<String, Vec<PathBuf>>>,
}

impl SystemFontIndex {
    /// Индекс, который при первом lookup просканирует стандартные пути
    /// текущей ОС. Конструктор не делает I/O — это случится при первом
    /// вызове `lookup_family` / `list_families`.
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

    fn index(&self) -> &HashMap<String, Vec<PathBuf>> {
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
        self.index().get(&key).cloned().unwrap_or_default()
    }

    fn list_families(&self) -> Vec<String> {
        let mut out: Vec<String> = self.index().keys().cloned().collect();
        out.sort();
        out
    }
}

fn build_index(dirs: &[PathBuf]) -> HashMap<String, Vec<PathBuf>> {
    let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for dir in dirs {
        scan_dir(dir, &mut index);
    }
    index
}

/// Рекурсивный обход директории. Битые файлы / файлы без `name` таблицы
/// тихо пропускаются — у системных шрифтов это норма (битмап-шрифты,
/// .pfb-файлы и прочее).
fn scan_dir(dir: &Path, index: &mut HashMap<String, Vec<PathBuf>>) {
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
        if let Some(family) = read_family(&path) {
            index
                .entry(family.to_ascii_lowercase())
                .or_default()
                .push(path);
        }
    }
}

fn is_supported_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf")
}

fn read_family(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let font = Font::parse(&bytes).ok()?;
    let name = font.name().ok()?;
    name.best_family().map(|s| s.to_owned())
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

    #[test]
    fn empty_index_when_dirs_dont_exist() {
        let idx = SystemFontIndex::with_dirs(vec![PathBuf::from("/definitely/does/not/exist/xyz")]);
        assert_eq!(idx.family_count(), 0);
        assert!(idx.list_families().is_empty());
        assert!(idx.lookup_family("Inter").is_empty());
    }

    #[test]
    fn finds_bundled_inter() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("assets")
            .join("fonts");
        let idx = SystemFontIndex::with_dirs(vec![assets]);
        assert_eq!(idx.family_count(), 1, "should find exactly one family in assets/fonts");
        let paths = idx.lookup_family("Inter");
        assert_eq!(paths.len(), 1, "Inter Regular registered once");
        assert!(paths[0].file_name().unwrap().to_string_lossy().contains("Inter"));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("assets")
            .join("fonts");
        let idx = SystemFontIndex::with_dirs(vec![assets]);
        assert_eq!(idx.lookup_family("inter").len(), 1);
        assert_eq!(idx.lookup_family("INTER").len(), 1);
        assert_eq!(idx.lookup_family("Inter").len(), 1);
    }

    #[test]
    fn unknown_family_returns_empty() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("assets")
            .join("fonts");
        let idx = SystemFontIndex::with_dirs(vec![assets]);
        assert!(idx.lookup_family("NoSuchFont").is_empty());
    }

    #[test]
    fn non_font_files_are_ignored() {
        // assets/fonts содержит и Inter-Regular.ttf, и OFL.txt — OFL.txt
        // не должен попасть в индекс.
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("assets")
            .join("fonts");
        let idx = SystemFontIndex::with_dirs(vec![assets]);
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
}
