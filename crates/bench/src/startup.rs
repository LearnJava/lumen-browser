//! Cold/warm start bench (PERF-4).
//!
//! Измеряет то, что пользователь чувствует при каждом запуске: **запуск exe →
//! первая страница готова**. В отличие от остальных бенчей этого крейта (они
//! гоняют pipeline-фазы in-process), здесь мы порождаем реальный собранный
//! бинарь `lumen` в headless-режиме `--screenshot` — тот же путь, что доводит
//! страницу до первого отрендеренного кадра, — и меряем настенное время от
//! спавна процесса до его выхода.
//!
//! Почему `--screenshot`, а не in-process: «холодный старт» — это не только
//! работа движка, но и загрузка 70-МБ бинаря (V8 статически слинкован) и его
//! DLL загрузчиком ОС, инициализация V8-платформы/изолята, установка DOM-шима —
//! всё то, что происходит ОДИН раз до первого кадра и растёт незаметно при
//! добавлении новых инициализаций на старте. In-process такой замер не поймал
//! бы стоимость загрузчика.
//!
//! Cold vs warm: первый спавн в прогоне — «холодный» (страничный кэш ОС для
//! бинаря и DLL холодный), последующие — «тёплые» (кэш прогрет). Настоящий
//! холодный старт (сброс кэша ОС) непортабелен, поэтому «cold» = первый спавн
//! прогона, «warm» = стационар остальных. Разница cold − warm_median и есть
//! стоимость прогрева загрузчика/инициализации.
//!
//! Запуск:
//!   cargo run -p lumen-bench --release -- --startup
//!   cargo run -p lumen-bench --release -- --startup --iters 15 --url samples/page.html
//!   cargo run -p lumen-bench --release -- --startup --json docs/perf/startup-runs/<date>.json
//!
//! Бинарь ищется: `--exe <путь>` > `$LUMEN_EXE` > `target/{dev-release,release,debug}/lumen[.exe]`.
//! Соберите его заранее: `cargo build -p lumen-shell --profile dev-release`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use lumen_core::json::JsonValue;

/// Число измеряемых спавнов по умолчанию (первый — cold, остальные — warm).
const DEFAULT_ITERS: usize = 12;
/// Страница по умолчанию — маленькая локальная (изолирует startup от сети/тяжёлого layout).
const DEFAULT_URL: &str = "samples/page.html";

/// Параметры прогона, разобранные из аргументов после `--startup`.
struct Opts {
    iters: usize,
    url: String,
    exe: Option<PathBuf>,
    json_out: Option<PathBuf>,
}

/// Точка входа: разбирает под-аргументы, гоняет бенч, печатает отчёт.
///
/// Возвращает код выхода процесса (0 — успех, 1 — бинарь не найден или один из
/// спавнов упал).
pub fn run(args: &[String]) -> i32 {
    let opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("startup bench: {e}");
            return 1;
        }
    };

    let exe = match locate_binary(opts.exe.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("startup bench: {e}");
            return 1;
        }
    };

    println!(
        "Lumen startup bench  exe={}  url={}  iters={}",
        exe.display(),
        opts.url,
        opts.iters
    );
    println!("mode: --screenshot (headless CPU render → первый кадр → exit)");
    println!();

    // Единый временный файл для скриншота — результат нам не нужен, важно лишь
    // что процесс проходит полный путь до первого кадра.
    let shot = std::env::temp_dir().join("lumen-startup-bench.png");

    let mut samples: Vec<Duration> = Vec::with_capacity(opts.iters);
    for i in 0..opts.iters {
        match spawn_once(&exe, &shot, &opts.url) {
            Ok(d) => {
                let tag = if i == 0 { "cold" } else { "warm" };
                println!("  spawn {:>2}/{}  {:>8}  ({tag})", i + 1, opts.iters, fmt(d));
                samples.push(d);
            }
            Err(e) => {
                eprintln!("startup bench: спавн {} упал: {e}", i + 1);
                let _ = std::fs::remove_file(&shot);
                return 1;
            }
        }
    }
    let _ = std::fs::remove_file(&shot);
    println!();

    let report = Report::from_samples(&samples);
    report.print();

    if let Some(path) = &opts.json_out {
        match write_json(path, &opts, &exe, &report) {
            Ok(()) => println!("\nJSON записан: {}", path.display()),
            Err(e) => {
                eprintln!("startup bench: не удалось записать JSON {}: {e}", path.display());
                return 1;
            }
        }
    }

    0
}

/// Один спавн: `lumen --screenshot <shot> <url>`, ждём выхода, меряем время.
///
/// stdout/stderr бинаря подавляются (баннер старта не должен засорять отчёт).
/// Ошибка — если процесс не запустился или вышел с ненулевым кодом.
fn spawn_once(exe: &Path, shot: &Path, url: &str) -> Result<Duration, String> {
    let start = Instant::now();
    let status = Command::new(exe)
        .arg("--screenshot")
        .arg(shot)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("не удалось запустить процесс: {e}"))?;
    let elapsed = start.elapsed();
    if !status.success() {
        return Err(format!("процесс вышел с кодом {status}"));
    }
    Ok(elapsed)
}

/// Агрегаты прогона: cold (первый спавн) и статистика warm (остальные).
struct Report {
    cold: Duration,
    /// Warm-статистика; `None`, если warm-спавнов не было (iters == 1).
    warm: Option<WarmStats>,
    n: usize,
}

/// Перцентили тёплых спавнов.
struct WarmStats {
    min: Duration,
    median: Duration,
    p95: Duration,
    max: Duration,
}

impl Report {
    /// Первый сэмпл — cold, остальные — warm. `samples` непуст (гарантирует caller).
    fn from_samples(samples: &[Duration]) -> Self {
        let cold = samples[0];
        let warm = if samples.len() > 1 {
            let mut rest: Vec<Duration> = samples[1..].to_vec();
            rest.sort();
            let n = rest.len();
            let p95_idx = ((n as f64 * 0.95).ceil() as usize).saturating_sub(1).min(n - 1);
            Some(WarmStats {
                min: rest[0],
                median: rest[n / 2],
                p95: rest[p95_idx],
                max: rest[n - 1],
            })
        } else {
            None
        };
        Self { cold, warm, n: samples.len() }
    }

    /// Печатает человекочитаемый отчёт.
    fn print(&self) {
        println!("  cold (first spawn)  {:>8}", fmt(self.cold));
        if let Some(w) = &self.warm {
            println!(
                "  warm ({} spawns)    min {:>8}  med {:>8}  p95 {:>8}  max {:>8}",
                self.n - 1,
                fmt(w.min),
                fmt(w.median),
                fmt(w.p95),
                fmt(w.max),
            );
            let delta = self.cold.saturating_sub(w.median);
            println!("  cold − warm_median  {:>8}  (стоимость прогрева загрузчика/init)", fmt(delta));
        }
    }
}

/// Формирует JSON-объект прогона и пишет его в файл (детерминированный порядок ключей).
fn write_json(path: &Path, opts: &Opts, exe: &Path, report: &Report) -> Result<(), String> {
    use std::collections::BTreeMap;

    let ms = |d: Duration| JsonValue::Number(d.as_secs_f64() * 1000.0);

    let mut root: BTreeMap<String, JsonValue> = BTreeMap::new();
    root.insert("bench".to_string(), JsonValue::String("startup".to_string()));
    root.insert("exe".to_string(), JsonValue::String(exe.display().to_string()));
    root.insert("url".to_string(), JsonValue::String(opts.url.clone()));
    root.insert("iters".to_string(), JsonValue::Number(report.n as f64));
    root.insert("cold_ms".to_string(), ms(report.cold));
    if let Some(w) = &report.warm {
        root.insert("warm_min_ms".to_string(), ms(w.min));
        root.insert("warm_median_ms".to_string(), ms(w.median));
        root.insert("warm_p95_ms".to_string(), ms(w.p95));
        root.insert("warm_max_ms".to_string(), ms(w.max));
        root.insert("cold_minus_warm_median_ms".to_string(), ms(report.cold.saturating_sub(w.median)));
    }

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, JsonValue::Object(root).to_string()).map_err(|e| e.to_string())
}

/// Разбирает под-аргументы после `--startup`.
fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut iters = DEFAULT_ITERS;
    let mut url = DEFAULT_URL.to_string();
    let mut exe: Option<PathBuf> = None;
    let mut json_out: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--iters" => {
                i += 1;
                let v = args.get(i).ok_or("--iters требует значение")?;
                iters = v.parse::<usize>().map_err(|_| format!("плохое --iters: {v}"))?;
                if iters == 0 {
                    return Err("--iters должно быть > 0".to_string());
                }
            }
            "--url" => {
                i += 1;
                url = args.get(i).ok_or("--url требует значение")?.clone();
            }
            "--exe" => {
                i += 1;
                exe = Some(PathBuf::from(args.get(i).ok_or("--exe требует значение")?));
            }
            "--json" => {
                i += 1;
                json_out = Some(PathBuf::from(args.get(i).ok_or("--json требует значение")?));
            }
            other => return Err(format!("неизвестный аргумент: {other}")),
        }
        i += 1;
    }

    Ok(Opts { iters, url, exe, json_out })
}

/// Ищет собранный бинарь `lumen`: `--exe` > `$LUMEN_EXE` > `target/{профили}/lumen[.exe]`.
fn locate_binary(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return if p.exists() {
            Ok(p.to_path_buf())
        } else {
            Err(format!("указанный --exe не найден: {}", p.display()))
        };
    }
    if let Ok(env) = std::env::var("LUMEN_EXE") {
        let p = PathBuf::from(env);
        if p.exists() {
            return Ok(p);
        }
    }

    // Корень workspace — на два уровня выше crates/bench (CARGO_MANIFEST_DIR).
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().and_then(|p| p.parent()).unwrap_or(&manifest);
    let exe_name = if cfg!(windows) { "lumen.exe" } else { "lumen" };
    for profile in ["dev-release", "release", "debug"] {
        let cand = root.join("target").join(profile).join(exe_name);
        if cand.exists() {
            return Ok(cand);
        }
    }
    Err(format!(
        "бинарь {exe_name} не найден. Соберите: cargo build -p lumen-shell --profile dev-release\n\
         (или укажите --exe <путь> / переменную LUMEN_EXE)"
    ))
}

/// Форматирует время (мс, ширина 8) — как в остальном бенче, но фиксированно в мс
/// (старты — сотни мс, разброс единиц измерения тут не нужен).
fn fmt(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults() {
        let o = parse_opts(&[]).expect("пустые аргументы ок");
        assert_eq!(o.iters, DEFAULT_ITERS);
        assert_eq!(o.url, DEFAULT_URL);
        assert!(o.exe.is_none());
        assert!(o.json_out.is_none());
    }

    #[test]
    fn parse_all_flags() {
        let args: Vec<String> = ["--iters", "5", "--url", "a.html", "--exe", "x/lumen", "--json", "o.json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let o = parse_opts(&args).expect("ок");
        assert_eq!(o.iters, 5);
        assert_eq!(o.url, "a.html");
        assert_eq!(o.exe.as_deref(), Some(Path::new("x/lumen")));
        assert_eq!(o.json_out.as_deref(), Some(Path::new("o.json")));
    }

    #[test]
    fn parse_rejects_zero_iters() {
        let args: Vec<String> = ["--iters", "0"].iter().map(|s| s.to_string()).collect();
        assert!(parse_opts(&args).is_err());
    }

    #[test]
    fn parse_rejects_unknown() {
        let args: Vec<String> = ["--bogus"].iter().map(|s| s.to_string()).collect();
        assert!(parse_opts(&args).is_err());
    }

    #[test]
    fn report_cold_only_single_sample() {
        let r = Report::from_samples(&[Duration::from_millis(300)]);
        assert_eq!(r.cold, Duration::from_millis(300));
        assert!(r.warm.is_none());
        assert_eq!(r.n, 1);
    }

    #[test]
    fn report_warm_percentiles() {
        let samples: Vec<Duration> = [500, 200, 210, 190, 205]
            .iter()
            .map(|&m| Duration::from_millis(m))
            .collect();
        let r = Report::from_samples(&samples);
        // cold — первый сэмпл, не минимум.
        assert_eq!(r.cold, Duration::from_millis(500));
        let w = r.warm.expect("есть warm");
        assert_eq!(w.min, Duration::from_millis(190));
        // warm = [200,210,190,205] → sort [190,200,205,210], median idx 2 = 205.
        assert_eq!(w.median, Duration::from_millis(205));
        assert_eq!(w.max, Duration::from_millis(210));
    }

    #[test]
    fn json_serializes_expected_keys() {
        let r = Report::from_samples(&[
            Duration::from_millis(300),
            Duration::from_millis(200),
            Duration::from_millis(210),
        ]);
        let opts = Opts {
            iters: 3,
            url: "samples/page.html".to_string(),
            exe: None,
            json_out: None,
        };
        let tmp = std::env::temp_dir().join("lumen-startup-bench-test.json");
        write_json(&tmp, &opts, Path::new("lumen.exe"), &r).expect("запись ок");
        let text = std::fs::read_to_string(&tmp).expect("чтение ок");
        let _ = std::fs::remove_file(&tmp);
        let v = lumen_core::json::parse(&text).expect("валидный JSON");
        assert_eq!(v.get("bench").and_then(|x| x.as_str()), Some("startup"));
        assert_eq!(v.get("iters").and_then(|x| x.as_number()), Some(3.0));
        assert_eq!(v.get("cold_ms").and_then(|x| x.as_number()), Some(300.0));
        assert!(v.get("warm_median_ms").is_some());
    }
}
