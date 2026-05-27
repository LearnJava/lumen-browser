//! Baseline-замеры pipeline-фаз на samples/page.html.
//!
//! Цели плана (cold start < 300 мс, RAM < 100 МБ на пустую вкладку) до сих
//! пор были лозунгами — без точки отсчёта невозможно отследить регрессии при
//! росте функциональности (Phase 1+ добавит шрифты, картинки, JS — каждая
//! фаза должна остаться в своём бюджете).
//!
//! Один бинарь, без criterion: запускаем pipeline `decode → parse html →
//! parse css → layout → paint::build_display_list` нужное число итераций,
//! печатаем агрегаты (min / median / mean / p95 / max) на фазу и total.
//! Также измеряем RSS (resident set size) для отслеживания регрессий памяти.
//!
//! Запуск:
//!   cargo run -p lumen-bench --release
//! Опционально число измерений (по умолчанию 100):
//!   LUMEN_BENCH_ITERS=500 cargo run -p lumen-bench --release
//!
//! Намеренно не используем `cargo bench` / nightly `test::Bencher`: первое
//! требует exception в политике зависимостей (criterion), второе — nightly
//! toolchain. Простой Instant-loop достаточен для baseline-цифр; статистики
//! и графики прикрутим, если упрёмся в необходимость различать шумовые
//! изменения < 5 %.

use std::hint::black_box;
use std::time::{Duration, Instant};

use lumen_core::geom::Size;
use lumen_dom::{Document, NodeData, NodeId};

const PAGE_HTML: &[u8] = include_bytes!("../../../samples/page.html");
const PAGE_CSS: &str = include_str!("../../../samples/page.css");
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

const DEFAULT_ITERS: usize = 100;
const WARMUP_ITERS: usize = 10;
const VIEWPORT: Size = Size {
    width: 1024.0,
    height: 720.0,
};

fn main() {
    let iters = std::env::var("LUMEN_BENCH_ITERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_ITERS);

    // Шрифт парсится один раз (как и в shell): TTF-tables не зависят от
    // конкретной страницы, и его парсинг — амортизированный cost холодного
    // старта браузера, а не per-page работа.
    let font = lumen_font::Font::parse(INTER_FONT).expect("Inter Regular parses");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer builds");

    // Один прогон до warm-up для проверки структуры: убеждаемся, что
    // pipeline действительно отрабатывает на ожидаемом объёме данных.
    // Если page.html / page.css пустые — это диагностируем сразу, до часовых
    // циклов измерений.
    let probe = run_pipeline(&measurer);
    println!("Lumen baseline bench");
    println!(
        "  page: {} bytes html + {} bytes css → {} DOM nodes, {} CSS rules, {} paint cmds",
        PAGE_HTML.len(),
        PAGE_CSS.len(),
        probe.dom_nodes,
        probe.css_rules,
        probe.paint_cmds
    );
    println!("  warmup: {WARMUP_ITERS} iters, measured: {iters} iters\n");

    for _ in 0..WARMUP_ITERS {
        black_box(run_pipeline(&measurer));
    }

    let mut samples = Samples::with_capacity(iters);
    for _ in 0..iters {
        samples.push(run_pipeline(&measurer));
    }

    print_phase("decode    ", &mut samples.decode);
    print_phase("parse_html", &mut samples.parse_html);
    print_phase("parse_css ", &mut samples.parse_css);
    print_phase("layout    ", &mut samples.layout);
    print_phase("paint     ", &mut samples.paint);
    println!();
    print_phase("TOTAL     ", &mut samples.total);
    println!();
    print_rss_stats(&mut samples.rss_bytes);
}

struct PipelineResult {
    decode: Duration,
    parse_html: Duration,
    parse_css: Duration,
    layout: Duration,
    paint: Duration,
    total: Duration,
    dom_nodes: usize,
    css_rules: usize,
    paint_cmds: usize,
    rss_bytes: u64,
}

/// Get current RSS (resident set size) in bytes.
/// Cross-platform: uses getrusage on Unix, GetProcessMemoryInfo on Windows.
fn get_rss_bytes() -> u64 {
    #[cfg(unix)]
    unsafe {
        let mut rusage = std::mem::zeroed::<libc::rusage>();
        if libc::getrusage(libc::RUSAGE_SELF, &mut rusage) == 0 {
            #[cfg(target_os = "macos")]
            {
                // macOS reports in bytes
                rusage.ru_maxrss as u64
            }
            #[cfg(not(target_os = "macos"))]
            {
                // Linux and other Unix systems report in kilobytes
                (rusage.ru_maxrss as u64) * 1024
            }
        } else {
            0
        }
    }
    #[cfg(target_os = "windows")]
    unsafe {
        use winapi::um::processthreadsapi::GetCurrentProcess;
        use winapi::um::psapi::GetProcessMemoryInfo;
        use winapi::um::psapi::PROCESS_MEMORY_COUNTERS;

        let mut pmc = std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

        let h_process = GetCurrentProcess();
        if GetProcessMemoryInfo(h_process, &mut pmc, pmc.cb) != 0 {
            pmc.WorkingSetSize as u64
        } else {
            0
        }
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        // Unsupported platform
        0
    }
}

fn run_pipeline(measurer: &lumen_paint::FontMeasurer<'_>) -> PipelineResult {
    let total_start = Instant::now();

    let t = Instant::now();
    let encoding = lumen_encoding::detect(PAGE_HTML, None);
    let source = lumen_encoding::decode(encoding, PAGE_HTML);
    let decode = t.elapsed();

    let t = Instant::now();
    let doc = lumen_html_parser::parse(&source);
    let parse_html = t.elapsed();

    // bench симулирует «в одном <style>-блоке» — это правильно для baseline,
    // потому что отделяет cost css-parser-а от cost загрузки внешних
    // stylesheet-ов (что в реальном shell стоит сетевого/файлового I/O).
    let t = Instant::now();
    let mut css = extract_style_blocks(&doc);
    css.push_str(PAGE_CSS);
    let sheet = lumen_css_parser::parse(&css);
    let parse_css = t.elapsed();

    let t = Instant::now();
    let layout = lumen_layout::layout_measured(&doc, &sheet, VIEWPORT, measurer);
    let layout_t = t.elapsed();

    let t = Instant::now();
    let list = lumen_paint::build_display_list(&layout);
    let paint = t.elapsed();

    let total = total_start.elapsed();
    let rss_bytes = get_rss_bytes();

    let dom_nodes = doc.len();
    let css_rules = sheet.rules.len();
    let paint_cmds = list.len();

    // black_box убеждает компилятор, что результаты «используются» —
    // иначе LTO в release-сборке может выкосить часть pipeline-а как мёртвый
    // код.
    black_box((doc, sheet, layout, list));

    PipelineResult {
        decode,
        parse_html,
        parse_css,
        layout: layout_t,
        paint,
        total,
        dom_nodes,
        css_rules,
        paint_cmds,
        rss_bytes,
    }
}

struct Samples {
    decode: Vec<Duration>,
    parse_html: Vec<Duration>,
    parse_css: Vec<Duration>,
    layout: Vec<Duration>,
    paint: Vec<Duration>,
    total: Vec<Duration>,
    rss_bytes: Vec<u64>,
}

impl Samples {
    fn with_capacity(cap: usize) -> Self {
        Self {
            decode: Vec::with_capacity(cap),
            parse_html: Vec::with_capacity(cap),
            parse_css: Vec::with_capacity(cap),
            layout: Vec::with_capacity(cap),
            paint: Vec::with_capacity(cap),
            total: Vec::with_capacity(cap),
            rss_bytes: Vec::with_capacity(cap),
        }
    }

    fn push(&mut self, r: PipelineResult) {
        self.decode.push(r.decode);
        self.parse_html.push(r.parse_html);
        self.parse_css.push(r.parse_css);
        self.layout.push(r.layout);
        self.paint.push(r.paint);
        self.total.push(r.total);
        self.rss_bytes.push(r.rss_bytes);
    }
}

fn print_phase(name: &str, samples: &mut [Duration]) {
    samples.sort();
    let n = samples.len();
    let min = samples[0];
    let max = samples[n - 1];
    let median = samples[n / 2];
    // p95: индекс по правилу `ceil(0.95 * n) - 1`, clamp в диапазон.
    let p95_idx = ((n as f64 * 0.95).ceil() as usize).saturating_sub(1).min(n - 1);
    let p95 = samples[p95_idx];
    let mean = mean_of(samples);
    println!(
        "  {name}  min {:>8}  med {:>8}  mean {:>8}  p95 {:>8}  max {:>8}",
        fmt(min),
        fmt(median),
        fmt(mean),
        fmt(p95),
        fmt(max)
    );
}

fn mean_of(samples: &[Duration]) -> Duration {
    let total: Duration = samples.iter().sum();
    total / (samples.len() as u32)
}

/// Форматирует время в выбранной единице (μs / ms) с шириной 8.
fn fmt(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        format!("{:.1} μs", ns as f64 / 1_000.0)
    } else {
        format!("{:.2} ms", ns as f64 / 1_000_000.0)
    }
}

/// Format bytes in appropriate unit (B / KB / MB / GB).
fn fmt_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes < KB {
        format!("{} B", bytes)
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.3} GB", bytes as f64 / GB as f64)
    }
}

fn print_rss_stats(samples: &mut [u64]) {
    if samples.is_empty() {
        return;
    }
    samples.sort();
    let n = samples.len();
    let min = samples[0];
    let max = samples[n - 1];
    let median = samples[n / 2];
    // p95: индекс по правилу `ceil(0.95 * n) - 1`, clamp в диапазон.
    let p95_idx = ((n as f64 * 0.95).ceil() as usize).saturating_sub(1).min(n - 1);
    let p95 = samples[p95_idx];
    let mean = samples.iter().sum::<u64>() / (n as u64);

    println!(
        "  RSS       min {:>8}  med {:>8}  mean {:>8}  p95 {:>8}  max {:>8}",
        fmt_bytes(min),
        fmt_bytes(median),
        fmt_bytes(mean),
        fmt_bytes(p95),
        fmt_bytes(max)
    );
}

fn extract_style_blocks(doc: &Document) -> String {
    let mut out = String::new();
    walk_style_blocks(doc, doc.root(), &mut out);
    out
}

fn walk_style_blocks(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
                out.push('\n');
            }
        }
        return;
    }
    for &child in &node.children {
        walk_style_blocks(doc, child, out);
    }
}
