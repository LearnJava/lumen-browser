//! Ярус 0 экспериментальной ветки: авто-проба wgpu-бэкенда при старте.
//!
//! Порядок кандидатов: **Vulkan** (быстрейший API на исправном драйвере:
//! прогретый кадр 7 мс против 450–950 мс на DX12, bugs/BUG-274-OPEN.md) →
//! **GL** (лучший idle-CPU на Intel Iris Plus) → **DX12** (резерв —
//! корректная картинка везде, но патология ~2.3 мс CPU на закрытие каждого
//! render pass). Для каждого кандидата рисуется пробный кадр (clear в
//! характерный цвет) прямо в поверхность окна, затем снимаются два сигнала:
//!
//! 1. **texture readback** — копия центра surface-текстуры в staging-буфер:
//!    проверяет, что рендер вообще пишет пиксели (ловит сломанный
//!    device / render pass);
//! 2. **захват презентации** — `PrintWindow(PW_CLIENTONLY |
//!    PW_RENDERFULLCONTENT)` клиентской области (Windows): проверяет, что
//!    presented-кадр дошёл до DWM. Именно этот сигнал ловит BUG-275
//!    (Vulkan-окно презентует белым при «исправном» по всем логам рендере —
//!    WSI-глюк драйвера Intel), который texture-readback может пропустить.
//!
//! Кандидат принимается, если презентация совпала с пробным цветом; при
//! недоступном захвате (не Windows / GDI-сбой) — если совпал readback.
//!
//! **Захват ненадёжен под нагрузкой (BUG-1073).** При одновременном старте
//! нескольких окон или параллельной сборке DWM не успевает скомпоновать
//! кадр, и захват видит белое у рабочего бэкенда. Поэтому: при `texture=ok`
//! захват ждётся дольше ([`CAPTURE_TRIES_TEXTURE_OK`]); если отклонены все
//! кандидаты, берётся кандидат с `texture=ok` ([`fallback_choice`]), а не
//! статическая цепочка; бюджет [`PROBE_BUDGET_MS`] обрывает пробу, когда
//! отклонённого кандидата поручает кэш. `Surface::configure` и в пробе, и в
//! рендере идёт через error scope ([`configure_checked`]): `Invalid surface`
//! — отказ кандидата, а не паника процесса.
//!
//! **Причина белого захвата — перекрытие окна (BUG-1073 срез 2).** Замер:
//! все белые захваты при рабочем readback — у окна, центр которого накрыт
//! соседним окном (другим `lumen` той же пачки); у открытого окна захват
//! сходится. `PrintWindow(PW_RENDERFULLCONTENT)` накрытого окна
//! flip-модельного swapchain не отдаёт его содержимое. Поэтому захват
//! накрытого окна не в счёт: сигнал `Unavailable`, решает readback, а
//! принятый так кандидат в кэш не пишется. Цена: машина с настоящим
//! BUG-275, чьё окно накрыто при старте, получит Vulkan на этот запуск —
//! без записи в кэш, следующий запуск пробует снова.
//!
//! **Устройство победителя переходит рендеру (BUG-1073 срез 3).** Проба
//! открывает кандидата тем же инстансом ([`renderer_instance_descriptor`])
//! и с теми же лимитами ([`window_device_limits`]), что рендер, и отдаёт
//! surface/adapter/device принятого кандидата в [`ProbeOutcome::gpu`]:
//! `Renderer::new_async` только переконфигурирует поверхность, а не
//! открывает бэкенд второй раз (`request_adapter` + `request_device` под
//! нагрузкой — 1–2.2 с драйвера). Отклонённые кандидаты закрываются до
//! следующего: две живые поверхности разных API на одном окне — риск
//! `Invalid surface`.
//!
//! **Пробный цвет закрывается сразу (BUG-1073 срез 4).** Принятый кандидат
//! презентует один белый кадр ([`present_neutral`]) до возврата из
//! [`pick_backend`]: без этого синий пробный кадр стоял до первого кадра
//! рендера — под нагрузкой ещё 1.6–3.7 с после выбора бэкенда.
//!
//! Управление:
//! - `WGPU_BACKEND=...` — проба пропускается, env-выбор главнее;
//! - `LUMEN_NO_BACKEND_PROBE=1` — проба выключена, работает статическая
//!   цепочка DX12 → Vulkan → GL (поведение до яруса 0);
//! - `LUMEN_FRAME_LOG=1` — подробный лог сигналов по каждому кандидату.
//!
//! Побочный эффект: на время пробы (~0.2–1 с на кандидата, до ~1.7 с суммарно
//! на первом запуске) окно показывает кадр(ы) пробного цвета — осознанная
//! плата за автоматический выбор API.
//!
//! **Кэш результата (BUG-274, "Probe 1.7с — кэшировать выбранный API между
//! запусками").** Кандидат, принятый в прошлый раз, пробуется первым при
//! следующем запуске — на типичной машине (одна и та же GPU/драйвер) это
//! сокращает пробу с 3 кандидатов до 1 (~1.7с → ~0.4–0.5с). Кэш не меняет
//! правильность выбора: принятый кандидат всё равно проходит ту же полную
//! валидацию (readback + захват презентации через DWM) — устаревший или
//! неверный кэш просто отклоняется пробой и проба сама скатывается на
//! оставшихся двух кандидатов в обычном порядке (self-healing, не риск).
//!
//! **Самолечение работает только вниз по порядку — и это чинилось в BUG-405
//! срезе 14.** Кандидат, принятый прошлый раз, пробуется первым и проходит,
//! поэтому стоящие ВПЕРЕДИ него кандидаты не пробуются больше никогда: одно
//! отклонение Vulkan'а (глюк DWM, старый драйвер) навсегда сажало машину на
//! DX12, который на той же Intel Iris Plus стоит вдвое дороже по кадру
//! прокрутки (`encode` 33 против 11.8 мс за прогон, сумма кадров 116 против
//! 53). Поэтому кэш хранит не только победителя, но и ключ окружения, в
//! котором он победил (адаптер, драйвер, версия Lumen): при расхождении
//! ключа проба перепроверяет кандидатов впереди победителя, а запись старого
//! формата (одно слово) права пропускать кандидатов не даёт вовсе.
//!
//! Файл — `<exe_dir>/data/paint/backend_probe.txt`, формат v2 — строки
//! `winner=`/`adapter=`/`driver=`/`app=`. Тот же портативный
//! конвенция, что `shell::adblock::browser_data_dir` (только папка браузера,
//! никогда OS-каталоги) — paint не может зависеть от shell (архитектурное
//! направление `lumen-core → … → paint → shell`), поэтому здесь свой
//! маленький самодостаточный резолвер `<exe_dir>/data`.

use std::sync::Arc;
use std::time::Instant;

use winit::window::Window;

use crate::renderer::{renderer_instance_descriptor, window_device_limits};

/// Итог пробы: выбранный бэкенд и, если он принят полной пробой, уже
/// открытое им устройство.
pub struct ProbeOutcome {
    /// Выбранный бэкенд — первым в цепочке рендера.
    pub backends: wgpu::Backends,
    /// Surface/adapter/device принятого кандидата (BUG-1073 срез 3). `None`
    /// — кандидат выбран без живого устройства (все отклонены, взят по
    /// readback): рендер открывает бэкенд сам.
    pub(crate) gpu: Option<ProbedGpu>,
}

/// Бэкенд, открытый пробой: поверхность окна сконфигурирована пробным
/// форматом, рендер переконфигурирует её под себя.
pub(crate) struct ProbedGpu {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    /// Конфигурация, с которой проба оставила поверхность: совпадёт с
    /// нужной рендеру — повторный `configure` (пересоздание swapchain,
    /// сотни мс под нагрузкой) не нужен.
    pub(crate) config: wgpu::SurfaceConfiguration,
}

/// Пробный цвет кадра, линейные компоненты 0..1. Выбран далёким и от белого
/// (симптом BUG-275), и от чёрного (пустой захват), с попарно различными
/// каналами — перепутанный порядок каналов не даст ложного совпадения.
const PROBE_COLOR: wgpu::Color = wgpu::Color { r: 0.25, g: 0.55, b: 0.85, a: 1.0 };

/// Цвет кадра, которым принятый кандидат закрывает пробный до первого кадра
/// рендера (BUG-1073 срез 4). Белый — фон страницы по умолчанию и то, что
/// окно показывает до пробы.
const NEUTRAL_COLOR: wgpu::Color = wgpu::Color::WHITE;

/// Допуск сравнения каналов (байты). Покрывает округление формата,
/// dithering DWM и лёгкие цветовые преобразования драйвера.
const TOLERANCE: i32 = 45;

/// Ширина региона readback (64 px × 4 байта = 256 = COPY_BYTES_PER_ROW_ALIGNMENT,
/// строка не требует паддинга).
const READBACK_W: u32 = 64;
/// Высота региона readback.
const READBACK_H: u32 = 16;

/// Пауза между попытками захвата презентации: DWM компонует кадр асинхронно.
const CAPTURE_STEP_MS: u64 = 120;
/// Попыток захвата, когда readback не подтвердил пробный цвет.
const CAPTURE_TRIES: u32 = 3;
/// Попыток захвата, когда readback пробный цвет подтвердил (`texture=ok`),
/// а захват ещё нет — BUG-1073: под нагрузкой DWM компонует кадр дольше
/// 360 мс, и рабочий бэкенд отклонялся как `present=WHITE`. ~1.2 с суммарно;
/// на машине с настоящим BUG-275 это разовая доплата (дальше выручает кэш).
const CAPTURE_TRIES_TEXTURE_OK: u32 = 10;

/// Бюджет пробы (BUG-1073): окно всё время пробы стоит пробным цветом
/// (зафиксировано 13–36 с на трёх отклонённых кандидатах подряд). Когда он
/// исчерпан, а отклонённый кандидат поручен кэшем ([`vouched_by_cache`]),
/// остальные не пробуются. Без поручительства бюджет не действует: на машине
/// с BUG-275 `present=WHITE texture=ok` у Vulkan — настоящий отказ, и
/// пропустить DX12 значило бы оставить белое окно.
const PROBE_BUDGET_MS: u128 = 4_000;

/// Результат одного сигнала пробы.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Signal {
    /// Пиксели совпали с пробным цветом.
    Match,
    /// Пиксели равномерно белые — симптом BUG-275.
    White,
    /// Пиксели есть, но не пробный цвет (средний RGB в байтах).
    Other([u8; 3]),
    /// Сигнал снять не удалось (нет COPY_SRC / GDI-сбой / не Windows).
    Unavailable,
}

impl Signal {
    /// Короткая метка для лога.
    fn label(self) -> String {
        match self {
            Signal::Match => "ok".into(),
            Signal::White => "WHITE".into(),
            Signal::Other([r, g, b]) => format!("other({r},{g},{b})"),
            Signal::Unavailable => "n/a".into(),
        }
    }
}

/// Ожидаемые байты канала для линейного значения `v` с учётом sRGB-кодирования
/// формата поверхности: non-sRGB формат хранит значение как есть (`v*255`),
/// sRGB-формат кодирует линейное значение в sRGB-байт.
fn expected_byte(v: f64, srgb: bool) -> u8 {
    let encoded = if srgb {
        if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
    } else {
        v
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Классифицирует средний цвет `avg` (порядок RGB) против пробного цвета.
fn classify(avg: [u8; 3], srgb: bool) -> Signal {
    let expected = [
        expected_byte(PROBE_COLOR.r, srgb),
        expected_byte(PROBE_COLOR.g, srgb),
        expected_byte(PROBE_COLOR.b, srgb),
    ];
    let matches = avg
        .iter()
        .zip(expected.iter())
        .all(|(&a, &e)| (i32::from(a) - i32::from(e)).abs() <= TOLERANCE);
    if matches {
        Signal::Match
    } else if avg.iter().all(|&c| c >= 240) {
        Signal::White
    } else {
        Signal::Other(avg)
    }
}

/// Отчёт пробы одного кандидата.
struct CandidateReport {
    /// Имя адаптера, каким его сообщил wgpu.
    adapter: String,
    /// Драйвер адаптера (`driver` + `driver_info`) — ключ инвалидации кэша
    /// (BUG-405 срез 14): обновление драйвера меняет исход пробы, а имя
    /// адаптера при этом не меняется.
    driver: String,
    /// Сигнал texture readback.
    texture: Signal,
    /// Сигнал захвата презентации.
    present: Signal,
    /// Захват не в счёт: окно накрыто чужим окном (BUG-1073). Кандидат,
    /// принятый по одному readback, в кэш не пишется — кэш «поручается»
    /// только за полную пробу.
    covered: bool,
    /// Разбивка времени кандидата по фазам (BUG-1073).
    phases: PhaseTimes,
    /// Открытый кандидатом бэкенд — рендер переиспользует его, если
    /// кандидат принят (BUG-1073 срез 3). У отклонённого закрывается сразу.
    gpu: Option<ProbedGpu>,
}

/// Время фаз пробы одного кандидата, мс. Без разбивки 24-секундный DX12
/// (BUG-1073) не локализовать: ожидание захвата — лишь 3×120 мс из них.
#[derive(Clone, Copy, Default, Debug)]
struct PhaseTimes {
    /// `create_surface` + `request_adapter`.
    adapter: u128,
    /// `request_device` + `configure`.
    device: u128,
    /// Два пробных кадра: `get_current_texture` → submit → present → readback.
    frames: u128,
    /// Ожидание и захват презентации.
    capture: u128,
}

impl PhaseTimes {
    /// Метка для `[probe]`-строки.
    fn label(self) -> String {
        format!(
            "adapter {} / device {} / frames {} / capture {}",
            self.adapter, self.device, self.frames, self.capture
        )
    }
}

/// Кандидат принят пробой: презентация совпала с пробным цветом, либо
/// захват недоступен, а readback совпал.
fn is_accepted(present: Signal, texture: Signal) -> bool {
    matches!(
        (present, texture),
        (Signal::Match, _) | (Signal::Unavailable, Signal::Match)
    )
}

/// Отклонённый пробой кандидат.
#[derive(Clone, Debug)]
struct Rejected {
    backends: wgpu::Backends,
    name: &'static str,
    /// Сигнал readback; `Unavailable`, если кандидат не открылся вовсе.
    texture: Signal,
    /// Адаптер и драйвер — `None`, если кандидат не открылся.
    adapter: Option<(String, String)>,
}

/// Отклонённый кандидат, которого «поручает» кэш (BUG-1073): readback
/// подтвердил пробный цвет, и этот же кандидат на том же адаптере, драйвере
/// и версии уже проходил полную пробу. Значит, GPU рисует верно, а не
/// сошёлся только захват презентации (окно накрыто, DWM под нагрузкой).
/// Чистая функция — тестируется без wgpu-контекста.
fn vouched_by_cache<'a>(rejected: &'a [Rejected], cache: Option<&ProbeCache>) -> Option<&'a Rejected> {
    let cache = cache.filter(|c| c.app == env!("CARGO_PKG_VERSION"))?;
    let winner = backend_by_name(&cache.winner)?;
    rejected.iter().find(|r| {
        r.backends == winner
            && r.texture == Signal::Match
            && r.adapter.as_ref().is_some_and(|(a, d)| *a == cache.adapter && *d == cache.driver)
    })
}

/// Выбор, когда отклонены все кандидаты: поручённый кэшем, иначе первый в
/// порядке пробы с `texture=ok`. Если захват не сошёлся ни у одного
/// кандидата, включая тот, что на этой машине заведомо работает, сломан
/// именно захват, а readback — единственный честный сигнал (BUG-1073,
/// Intel UHD: `WHITE` у всех трёх при `texture=ok` у Vulkan и DX12).
/// `None` — такого нет, работает статическая цепочка.
fn fallback_choice<'a>(rejected: &'a [Rejected], cache: Option<&ProbeCache>) -> Option<&'a Rejected> {
    vouched_by_cache(rejected, cache).or_else(|| rejected.iter().find(|r| r.texture == Signal::Match))
}

/// `Surface::configure` без паники: ошибка валидации (`Invalid surface` —
/// драйвер не отдал swapchain этому окну, BUG-1073) ловится error scope'ом и
/// возвращается как `Err`, а не уходит в необработанный обработчик wgpu,
/// который паникует. Вызывающий переходит к следующему бэкенду.
pub(crate) async fn configure_checked(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> Result<(), String> {
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(device, config);
    match device.pop_error_scope().await {
        None => Ok(()),
        Some(e) => Err(format!("configure: {e}")),
    }
}

/// `true`, если проба выключена (`LUMEN_NO_BACKEND_PROBE=1`) или бэкенд
/// уже выбран явно (`WGPU_BACKEND`).
fn probe_disabled() -> bool {
    std::env::var("LUMEN_NO_BACKEND_PROBE").is_ok_and(|v| v == "1")
        || std::env::var("WGPU_BACKEND").is_ok_and(|v| !v.trim().is_empty())
}

/// `<exe_dir>/data/paint/backend_probe.txt` — портативное хранилище кэша
/// пробы (не OS-каталог), см. модульную документацию. Падает на
/// относительный `data/paint/...`, если путь исполняемого файла недоступен.
fn cache_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("data").join("paint")))
        .unwrap_or_else(|| std::path::PathBuf::from("data/paint"))
        .join("backend_probe.txt")
}

/// Запись кэша пробы (формат v2, BUG-405 срез 14).
///
/// Кроме победителя хранит ключ окружения, в котором он победил: имя
/// адаптера, строку драйвера и версию приложения. Победитель без ключа
/// (формат v1 — одно слово в файле) не даёт права пропускать кандидатов:
/// именно так «кандидат отклонён один раз» превращалось в «кандидат не
/// пробуется никогда».
#[derive(Clone, PartialEq, Eq, Debug)]
struct ProbeCache {
    /// Короткое имя принятого кандидата: `Vulkan` / `GL` / `DX12`.
    winner: String,
    /// Имя адаптера, на котором победитель прошёл пробу.
    adapter: String,
    /// Драйвер этого адаптера (`driver` + `driver_info`).
    driver: String,
    /// Версия Lumen, в которой снят результат.
    app: String,
}

/// Разбирает содержимое файла кэша. `None` — пусто, мусор или формат v1
/// (одно слово): и то, и другое означает «улик нет, проба идёт полностью».
fn parse_cache(text: &str) -> Option<ProbeCache> {
    let mut winner = None;
    let mut adapter = None;
    let mut driver = None;
    let mut app = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "winner" => winner = Some(value),
            "adapter" => adapter = Some(value),
            "driver" => driver = Some(value),
            "app" => app = Some(value),
            _ => {}
        }
    }
    let cache = ProbeCache {
        winner: winner?,
        adapter: adapter?,
        driver: driver?,
        app: app?,
    };
    backend_by_name(&cache.winner)?;
    Some(cache)
}

/// Собирает содержимое файла кэша.
fn serialize_cache(cache: &ProbeCache) -> String {
    format!(
        "winner={}\nadapter={}\ndriver={}\napp={}\n",
        cache.winner, cache.adapter, cache.driver, cache.app
    )
}

/// Короткое имя кандидата → набор бэкендов wgpu.
fn backend_by_name(name: &str) -> Option<wgpu::Backends> {
    match name {
        "Vulkan" => Some(wgpu::Backends::VULKAN),
        "GL" => Some(wgpu::Backends::GL),
        "DX12" => Some(wgpu::Backends::DX12),
        _ => None,
    }
}

/// Читает кэш прошлой пробы.
fn read_cache() -> Option<ProbeCache> {
    parse_cache(&std::fs::read_to_string(cache_path()).ok()?)
}

/// Сохраняет результат пробы для следующего запуска.
/// Best-effort: любая ошибка ФС (нет прав на запись в `data/`, диск только
/// для чтения) молча игнорируется — следующий запуск просто снова пройдёт
/// полную пробу без кэша, поведение не хуже, чем до этого среза.
fn write_cache(cache: &ProbeCache) {
    let path = cache_path();
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return;
    }
    let _ = std::fs::write(path, serialize_cache(cache));
}

/// Переставляет `cached` (если он есть среди `candidates`) на первое место,
/// не трогая относительный порядок остальных. Чистая функция — тестируется
/// без wgpu-контекста.
fn reorder_by_cache<T: Copy>(
    candidates: [(wgpu::Backends, T); 3],
    cached: Option<wgpu::Backends>,
) -> Vec<(wgpu::Backends, T)> {
    let mut order: Vec<(wgpu::Backends, T)> = candidates.to_vec();
    if let Some(cached) = cached
        && let Some(pos) = order.iter().position(|(b, _)| *b == cached)
    {
        let hit = order.remove(pos);
        order.insert(0, hit);
    }
    order
}

/// Кандидаты, которые стоят в статическом порядке ВПЕРЕДИ победителя кэша:
/// их надо перепробовать, если окружение (адаптер / драйвер / версия
/// приложения) изменилось с прошлого запуска — BUG-405 срез 14.
fn candidates_before<T: Copy>(
    candidates: [(wgpu::Backends, T); 3],
    winner: wgpu::Backends,
) -> Vec<(wgpu::Backends, T)> {
    candidates
        .iter()
        .take_while(|(b, _)| *b != winner)
        .copied()
        .collect()
}

/// Авто-проба бэкендов: возвращает первый кандидат из цепочки
/// Vulkan → GL → DX12 (или закэшированный кандидат первым, см. модульную
/// документацию), чей пробный кадр реально виден на экране.
///
/// `None` — проба выключена/неприменима (env-override, не Windows) или все
/// кандидаты провалились; вызывающий код использует статическую цепочку.
pub async fn pick_backend(window: &Arc<Window>) -> Option<ProbeOutcome> {
    // BUG-275 — специфика Windows (DWM/WSI); на других ОС проба не нужна,
    // а пробный цветной кадр в окне — неоправданный побочный эффект.
    if !cfg!(target_os = "windows") || probe_disabled() {
        return None;
    }
    let started = Instant::now();
    let candidates: [(wgpu::Backends, &str); 3] = [
        (wgpu::Backends::VULKAN, "Vulkan"),
        (wgpu::Backends::GL, "GL"),
        (wgpu::Backends::DX12, "DX12"),
    ];
    let cache = read_cache();
    let cached_backend = cache
        .as_ref()
        .filter(|c| c.app == env!("CARGO_PKG_VERSION"))
        .and_then(|c| backend_by_name(&c.winner));
    let order = reorder_by_cache(candidates, cached_backend);

    let mut winner: Option<(wgpu::Backends, &str, CandidateReport)> = None;
    let mut rejected: Vec<Rejected> = Vec::new();
    let mut remaining = order.into_iter();
    for (backends, name) in remaining.by_ref() {
        match probe_one(window, backends, name).await {
            Ok(rep) => {
                winner = Some((backends, name, rep));
                break;
            }
            Err((texture, adapter)) => rejected.push(Rejected { backends, name, texture, adapter }),
        }
        if started.elapsed().as_millis() >= PROBE_BUDGET_MS
            && vouched_by_cache(&rejected, cache.as_ref()).is_some()
        {
            break;
        }
    }
    let Some((backends, name, rep)) = winner else {
        let skipped: Vec<&str> = remaining.map(|(_, n)| n).collect();
        let why = if skipped.is_empty() {
            "все кандидаты отклонены".to_string()
        } else {
            format!("бюджет {PROBE_BUDGET_MS} мс исчерпан, не пробовались: {}", skipped.join(", "))
        };
        // BUG-1073: readback подтвердил пробный цвет — GPU рисует верно, не
        // сошёлся только захват презентации. Такой кандидат лучше
        // статической цепочки; кэш не переписывается — следующий запуск
        // пробует заново.
        if let Some(r) = fallback_choice(&rejected, cache.as_ref()) {
            eprintln!(
                "[probe] {why} за {} мс — беру {}: texture=ok, не сошёлся только захват",
                started.elapsed().as_millis(),
                r.name
            );
            return Some(ProbeOutcome { backends: r.backends, gpu: None });
        }
        eprintln!(
            "[probe] {why} за {} мс — статическая цепочка",
            started.elapsed().as_millis()
        );
        return None;
    };

    // BUG-405 срез 14: кэш пропускает кандидатов, стоящих в статическом
    // порядке впереди победителя, — но только пока окружение то же. Иначе
    // однажды отклонённый кандидат не пробуется больше НИКОГДА: на этой
    // машине так и вышло — закэшированный DX12 принимался первым, Vulkan
    // (первый по порядку, вдвое дешевле по кадру прокрутки) не пробовался
    // ни разу, хотя давно проходит пробу.
    let (mut backends, mut name, mut rep) = (backends, name, rep);
    let stale = cache.as_ref().is_none_or(|c| {
        c.adapter != rep.adapter || c.driver != rep.driver || c.app != env!("CARGO_PKG_VERSION")
    });
    if stale && cached_backend == Some(backends) {
        eprintln!(
            "[probe] окружение изменилось (адаптер/драйвер/версия) — \
             перепроверяю кандидатов впереди {name}"
        );
        // Устройство победителя закрывается до пробы других: вторая живая
        // поверхность другого API на том же окне — риск `Invalid surface`.
        // Если перепроверка не найдёт лучшего, рендер откроет его сам.
        rep.gpu = None;
        for (b, n) in candidates_before(candidates, backends) {
            if started.elapsed().as_millis() >= PROBE_BUDGET_MS {
                break;
            }
            if let Ok(better) = probe_one(window, b, n).await {
                backends = b;
                name = n;
                rep = better;
                break;
            }
        }
    }

    // BUG-1073 срез 4: пробный цвет свою работу сделал — сразу закрыть его
    // нейтральным кадром, а не держать до первого кадра рендера (под
    // нагрузкой ещё 1.5–5 с: сборка пайплайнов, первая страница).
    if let Some(gpu) = rep.gpu.as_ref() {
        let t = Instant::now();
        match present_neutral(gpu) {
            Ok(()) if crate::frame_log_enabled() => {
                eprintln!("[probe]   нейтральный кадр: {} мс", t.elapsed().as_millis());
            }
            Ok(()) => {}
            Err(e) => eprintln!("[probe]   нейтральный кадр не показан: {e}"),
        }
    }
    eprintln!("[probe] бэкенд выбран за {} мс: {name}", started.elapsed().as_millis());
    // BUG-1073: принятый по readback при накрытом окне — не полная проба;
    // запиши его в кэш, и бюджет пробы следующего запуска «поручился» бы
    // за кандидата, которого захват ни разу не видел (BUG-275: белое окно).
    if !rep.covered {
        write_cache(&ProbeCache {
            winner: name.to_string(),
            adapter: rep.adapter.clone(),
            driver: rep.driver.clone(),
            app: env!("CARGO_PKG_VERSION").to_string(),
        });
    }
    Some(ProbeOutcome { backends, gpu: rep.gpu })
}

/// Пробует одного кандидата и печатает его отчёт. `Ok` — принят; `Err` —
/// отклонён: сигнал readback и (адаптер, драйвер), если кандидат открылся
/// (для [`fallback_choice`]).
async fn probe_one(
    window: &Arc<Window>,
    backends: wgpu::Backends,
    name: &str,
) -> Result<CandidateReport, (Signal, Option<(String, String)>)> {
    let t0 = Instant::now();
    match probe_candidate(window, backends).await {
        Ok(rep) => {
            let accepted = is_accepted(rep.present, rep.texture);
            eprintln!(
                "[probe] {name}: present={} texture={} adapter=\"{}\" ({} мс: {}) — {}",
                rep.present.label(),
                rep.texture.label(),
                rep.adapter,
                t0.elapsed().as_millis(),
                rep.phases.label(),
                if accepted { "ПРИНЯТ" } else { "отклонён" },
            );
            if accepted { Ok(rep) } else { Err((rep.texture, Some((rep.adapter, rep.driver)))) }
        }
        Err(e) => {
            eprintln!("[probe] {name}: недоступен ({e}, {} мс)", t0.elapsed().as_millis());
            Err((Signal::Unavailable, None))
        }
    }
}

/// Пробует один бэкенд: instance → surface → adapter → device → 2 кадра
/// clear-ом пробного цвета → readback + захват презентации.
async fn probe_candidate(
    window: &Arc<Window>,
    backends: wgpu::Backends,
) -> Result<CandidateReport, String> {
    let mut phases = PhaseTimes::default();
    let t_phase = Instant::now();
    // Инстанс — как у рендера (флаги BUG-406): устройство победителя
    // переходит рендеру (BUG-1073 срез 3). `with_env()` внутри не сменит
    // бэкенд: probe_disabled() уже гарантировал, что WGPU_BACKEND не задан.
    let instance = wgpu::Instance::new(&renderer_instance_descriptor(backends));
    let surface = instance
        .create_surface(window.clone())
        .map_err(|e| format!("create_surface: {e}"))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| format!("request_adapter: {e}"))?;
    phases.adapter = t_phase.elapsed().as_millis();
    let t_phase = Instant::now();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("lumen-device"),
            required_features: wgpu::Features::empty(),
            required_limits: window_device_limits(adapter.limits().max_texture_dimension_2d),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| format!("request_device: {e}"))?;

    let caps = surface.get_capabilities(&adapter);
    // Пустые caps — поверхность с этим адаптером несовместима; индексировать
    // `formats[0]`/`alpha_modes[0]` нельзя (паника вместо отказа кандидата).
    let (Some(&first_format), Some(&alpha_mode)) = (caps.formats.first(), caps.alpha_modes.first())
    else {
        return Err("surface: адаптер не отдал ни одного формата".into());
    };
    let format = caps
        .formats
        .iter()
        .find(|f| !f.is_srgb())
        .copied()
        .unwrap_or(first_format);
    let can_copy = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
    let size = window.inner_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let config = wgpu::SurfaceConfiguration {
        usage: if can_copy {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        },
        format,
        width,
        height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    configure_checked(&surface, &device, &config).await?;
    phases.device = t_phase.elapsed().as_millis();
    let t_phase = Instant::now();

    // Порядок байтов текселя для readback-классификации.
    let byte_order: Option<[usize; 3]> = match format {
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => Some([0, 1, 2]),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => Some([2, 1, 0]),
        _ => None,
    };
    let do_readback =
        can_copy && byte_order.is_some() && width >= READBACK_W && height >= READBACK_H;

    // Два кадра: первый прогревает swapchain, у второго снимаем readback.
    let mut texture = Signal::Unavailable;
    for frame_idx in 0..2 {
        let frame = surface
            .get_current_texture()
            .map_err(|e| format!("get_current_texture: {e}"))?;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("probe-encoder"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("probe-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(PROBE_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        let staging = if frame_idx == 1 && do_readback {
            let staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("probe-readback"),
                size: u64::from(READBACK_W * 4 * READBACK_H),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: (width - READBACK_W) / 2,
                        y: (height - READBACK_H) / 2,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(READBACK_W * 4),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: READBACK_W,
                    height: READBACK_H,
                    depth_or_array_layers: 1,
                },
            );
            Some(staging)
        } else {
            None
        };
        queue.submit([encoder.finish()]);
        frame.present();

        if let Some(staging) = staging {
            texture = read_staging(&device, &staging, byte_order, format.is_srgb())?;
        }
    }

    phases.frames = t_phase.elapsed().as_millis();
    let t_phase = Instant::now();

    // DWM компонует презентованный кадр асинхронно — даём ему время и
    // перепроверяем захват, принимая первый Match. Если readback пробный
    // цвет подтвердил, ждём дольше (BUG-1073): отказ по одному захвату при
    // рабочем GPU отклонял исправный бэкенд.
    let tries = if texture == Signal::Match { CAPTURE_TRIES_TEXTURE_OK } else { CAPTURE_TRIES };
    let mut present = Signal::Unavailable;
    let mut covered = false;
    for _ in 0..tries {
        std::thread::sleep(std::time::Duration::from_millis(CAPTURE_STEP_MS));
        match capture_present(window, format.is_srgb()) {
            Some(sig) => {
                present = sig;
                if sig == Signal::Match {
                    break;
                }
            }
            None => {
                present = Signal::Unavailable;
                break;
            }
        }
        // BUG-1073: центр окна накрыт чужим окном — `PrintWindow` отдаёт
        // белое при рабочем GPU (замер: все белые захваты под нагрузкой —
        // у окна, накрытого соседним окном Lumen). Такой захват о
        // презентации ничего не говорит: сигнал недоступен, решает
        // readback, как на системе без захвата.
        if let Some((true, detail)) = window_cover(window) {
            eprintln!("[probe]   захват {} не в счёт, окно накрыто: {detail}", present.label());
            present = Signal::Unavailable;
            covered = true;
            break;
        }
    }

    phases.capture = t_phase.elapsed().as_millis();
    // Состояние окна: всегда при несошедшемся захвате открытого окна
    // (BUG-275 — окно наверху, на экране белое), а под `LUMEN_FRAME_LOG` —
    // у каждого кандидата, чтобы видеть, как часто захват накрытого окна
    // всё же сходится.
    if !covered
        && (!matches!(present, Signal::Match | Signal::Unavailable) || crate::frame_log_enabled())
        && let Some((_, detail)) = window_cover(window)
    {
        eprintln!("[probe]   захват {}: {detail}", present.label());
    }
    let info = adapter.get_info();
    Ok(CandidateReport {
        adapter: info.name,
        driver: format!("{} {}", info.driver, info.driver_info),
        texture,
        present,
        covered,
        phases,
        gpu: Some(ProbedGpu { surface, adapter, device, queue, config }),
    })
}

/// Презентует кадр [`NEUTRAL_COLOR`] поверх пробного (BUG-1073 срез 4).
///
/// Пробный цвет нужен только на время захвата. Дальше окно стоит им до
/// первого кадра рендера, а под нагрузкой это 1.5–5 с после выбора бэкенда
/// (замер: фоновые пайплайны, первая страница на голодающем рендер-потоке) —
/// пользователь видит «синий экран» и считает, что браузер завис. Кадр
/// ставится устройством пробы, ожидания GPU нет: `present` уходит в очередь.
fn present_neutral(gpu: &ProbedGpu) -> Result<(), String> {
    let frame = gpu
        .surface
        .get_current_texture()
        .map_err(|e| format!("get_current_texture: {e}"))?;
    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("probe-neutral-encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("probe-neutral"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(NEUTRAL_COLOR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    gpu.queue.submit([encoder.finish()]);
    frame.present();
    Ok(())
}

/// Читает staging-буфер readback-а и классифицирует средний цвет региона.
fn read_staging(
    device: &wgpu::Device,
    staging: &wgpu::Buffer,
    byte_order: Option<[usize; 3]>,
    srgb: bool,
) -> Result<Signal, String> {
    let Some(order) = byte_order else {
        return Ok(Signal::Unavailable);
    };
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::Wait)
        .map_err(|e| format!("device.poll: {e}"))?;
    rx.recv()
        .map_err(|_| "readback: канал map_async оборвался".to_string())?
        .map_err(|e| format!("map_async: {e}"))?;
    let mapped = slice.get_mapped_range();
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for texel in mapped.chunks_exact(4) {
        for (i, &o) in order.iter().enumerate() {
            sum[i] += u64::from(texel[o]);
        }
        count += 1;
    }
    drop(mapped);
    staging.unmap();
    if count == 0 {
        return Ok(Signal::Unavailable);
    }
    let avg = [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ];
    Ok(classify(avg, srgb))
}

/// Захватывает презентованное содержимое клиентской области окна и
/// классифицирует средний цвет центрального блока 32×32.
///
/// `None` — захват недоступен (не Windows / GDI-сбой) — сигнал
/// [`Signal::Unavailable`] у вызывающего.
#[cfg(target_os = "windows")]
fn capture_present(window: &Window, srgb: bool) -> Option<Signal> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let hwnd = match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get() as *mut std::ffi::c_void,
        _ => return None,
    };
    let avg = win_capture::client_center_avg(hwnd)?;
    Some(classify(avg, srgb))
}

/// Заглушка захвата для не-Windows: сигнал недоступен.
#[cfg(not(target_os = "windows"))]
fn capture_present(_window: &Window, _srgb: bool) -> Option<Signal> {
    None
}

/// Перекрыто ли окно чужим окном и строка его состояния для лога —
/// для решения по несошедшемуся захвату (BUG-1073).
#[cfg(target_os = "windows")]
fn window_cover(window: &Window) -> Option<(bool, String)> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => {
            let cover = win_capture::cover(h.hwnd.get() as *mut std::ffi::c_void);
            Some((cover.covered, cover.detail))
        }
        _ => None,
    }
}

/// Заглушка для не-Windows: захвата нет, перекрытие не проверяется.
#[cfg(not(target_os = "windows"))]
fn window_cover(_window: &Window) -> Option<(bool, String)> {
    None
}

// ── Windows: PrintWindow-захват клиентской области ──────────────────────────

#[cfg(target_os = "windows")]
mod win_capture {
    use std::ffi::c_void;

    /// PW_CLIENTONLY | PW_RENDERFULLCONTENT — клиентская область с
    /// DWM-содержимым (GPU-swapchain), а не только GDI-поверхность.
    const PW_FLAGS: u32 = 0x1 | 0x2;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;

    /// RECT (windef.h).
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    /// BITMAPINFOHEADER (wingdi.h).
    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    /// BITMAPINFO (wingdi.h) — минимальная таблица цветов.
    #[repr(C)]
    struct BitmapInfo {
        bmi_header: BitmapInfoHeader,
        bmi_colors: [u32; 1],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClientRect(h_wnd: *mut c_void, rect: *mut Rect) -> i32;
        fn GetDC(h_wnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(h_wnd: *mut c_void, h_dc: *mut c_void) -> i32;
        fn PrintWindow(h_wnd: *mut c_void, h_dc: *mut c_void, flags: u32) -> i32;
        fn IsWindowVisible(h_wnd: *mut c_void) -> i32;
        fn IsIconic(h_wnd: *mut c_void) -> i32;
        fn GetForegroundWindow() -> *mut c_void;
        fn ClientToScreen(h_wnd: *mut c_void, point: *mut Point) -> i32;
        fn WindowFromPoint(point: Point) -> *mut c_void;
        fn GetAncestor(h_wnd: *mut c_void, flags: u32) -> *mut c_void;
        fn GetWindowThreadProcessId(h_wnd: *mut c_void, pid: *mut u32) -> u32;
        fn GetClassNameW(h_wnd: *mut c_void, name: *mut u16, max: i32) -> i32;
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmGetWindowAttribute(h_wnd: *mut c_void, attr: u32, value: *mut c_void, size: u32) -> i32;
    }

    /// POINT (windef.h).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: i32,
        y: i32,
    }

    /// `GA_ROOT` для `GetAncestor`.
    const GA_ROOT: u32 = 2;
    /// `DWMWA_CLOAKED` для `DwmGetWindowAttribute`.
    const DWMWA_CLOAKED: u32 = 14;
    /// `SRCCOPY` для `BitBlt`.
    const SRCCOPY: u32 = 0x00CC_0020;

    /// Кто лежит поверх центра клиентской области окна (BUG-1073).
    pub struct Cover {
        /// Центр клиентской области перекрыт чужим окном верхнего уровня.
        pub covered: bool,
        /// Строка для `[probe]`-лога: видимость, фокус, `cloaked`, владелец
        /// перекрывающего окна (pid, класс) и цвет экрана в центре.
        pub detail: String,
    }

    /// Состояние окна в момент несошедшегося захвата (BUG-1073). Замер
    /// показал: `PrintWindow` у окна, центр которого накрыт другим окном,
    /// отдаёт белое при рабочем GPU — захват в этом случае ничего не говорит
    /// о презентации. Отличает «окно накрыто» от BUG-275 (окно наверху,
    /// на экране белое).
    pub fn cover(hwnd: *mut c_void) -> Cover {
        // SAFETY: вызовы Win32/DWM с `hwnd` окна winit; выходные буферы —
        // локальные переменные нужного размера, длины переданы явно.
        unsafe {
            let visible = IsWindowVisible(hwnd) != 0;
            let iconic = IsIconic(hwnd) != 0;
            let foreground = GetForegroundWindow() == hwnd;
            let mut cloaked: u32 = 0;
            let cloak_hr = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            );
            let mut rect = Rect { left: 0, top: 0, right: 0, bottom: 0 };
            GetClientRect(hwnd, &raw mut rect);
            let mut center = Point { x: (rect.right - rect.left) / 2, y: (rect.bottom - rect.top) / 2 };
            ClientToScreen(hwnd, &raw mut center);
            let at_center = GetAncestor(WindowFromPoint(center), GA_ROOT);
            let covered = !at_center.is_null() && at_center != hwnd;
            let owner = if covered {
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(at_center, &raw mut pid);
                let mut class = [0u16; 64];
                let len = GetClassNameW(at_center, class.as_mut_ptr(), class.len() as i32);
                let class = String::from_utf16_lossy(&class[..usize::try_from(len).unwrap_or(0)]);
                format!(" cover=pid {pid} class \"{class}\"")
            } else {
                String::new()
            };
            let screen = screen_avg(center)
                .map_or_else(|| "n/a".to_string(), |[r, g, b]| format!("({r},{g},{b})"));
            let cloaked = if cloak_hr == 0 { cloaked.to_string() } else { format!("hr{cloak_hr:#x}") };
            Cover {
                covered,
                detail: format!(
                    "visible={visible} iconic={iconic} foreground={foreground} cloaked={cloaked} \
                     own_pid={} covered={covered}{owner} screen={screen}",
                    std::process::id()
                ),
            }
        }
    }

    /// Средний цвет блока 32×32 экрана вокруг `center` (экранные координаты):
    /// то, что видит пользователь, а не содержимое окна по `PrintWindow`.
    fn screen_avg(center: Point) -> Option<[u8; 3]> {
        const SIDE: i32 = 32;
        // SAFETY: паттерн `client_center_avg`: каждый handle проверяется,
        // ресурсы освобождаются до выхода; буфер пикселей — SIDE×SIDE×4.
        unsafe {
            let screen_dc = GetDC(std::ptr::null_mut());
            if screen_dc.is_null() {
                return None;
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            let bitmap = if mem_dc.is_null() {
                std::ptr::null_mut()
            } else {
                CreateCompatibleBitmap(screen_dc, SIDE, SIDE)
            };
            let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
            let mut ok = false;
            if !bitmap.is_null() {
                let old_obj = SelectObject(mem_dc, bitmap);
                if BitBlt(mem_dc, 0, 0, SIDE, SIDE, screen_dc, center.x - SIDE / 2, center.y - SIDE / 2, SRCCOPY)
                    != 0
                {
                    let mut bmi = dib_info(SIDE, SIDE);
                    ok = GetDIBits(
                        mem_dc,
                        bitmap,
                        0,
                        SIDE as u32,
                        pixels.as_mut_ptr().cast::<c_void>(),
                        &raw mut bmi,
                        DIB_RGB_COLORS,
                    ) > 0;
                }
                SelectObject(mem_dc, old_obj);
                DeleteObject(bitmap);
            }
            if !mem_dc.is_null() {
                DeleteDC(mem_dc);
            }
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            if !ok {
                return None;
            }
            let mut sum = [0u64; 3];
            for px in pixels.chunks_exact(4) {
                sum[0] += u64::from(px[2]);
                sum[1] += u64::from(px[1]);
                sum[2] += u64::from(px[0]);
            }
            let n = (SIDE * SIDE) as u64;
            Some([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8])
        }
    }

    /// Top-down 32-битный BITMAPINFO размера `w`×`h`.
    fn dib_info(w: i32, h: i32) -> BitmapInfo {
        BitmapInfo {
            bmi_header: BitmapInfoHeader {
                bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: w,
                bi_height: -h,
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: BI_RGB,
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            bmi_colors: [0],
        }
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(h_dc: *mut c_void) -> *mut c_void;
        fn CreateCompatibleBitmap(h_dc: *mut c_void, cx: i32, cy: i32) -> *mut c_void;
        #[allow(clippy::too_many_arguments)]
        fn BitBlt(
            h_dc: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            h_dc_src: *mut c_void,
            x1: i32,
            y1: i32,
            rop: u32,
        ) -> i32;
        fn SelectObject(h_dc: *mut c_void, h: *mut c_void) -> *mut c_void;
        fn GetDIBits(
            h_dc: *mut c_void,
            h_bm: *mut c_void,
            start: u32,
            c_lines: u32,
            lp_vbits: *mut c_void,
            lp_bmi: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn DeleteObject(ho: *mut c_void) -> i32;
        fn DeleteDC(h_dc: *mut c_void) -> i32;
    }

    /// Средний цвет (RGB) центрального блока 32×32 клиентской области окна,
    /// снятого через `PrintWindow`. `None` при любом GDI-сбое.
    pub fn client_center_avg(hwnd: *mut c_void) -> Option<[u8; 3]> {
        // SAFETY: все вызовы Win32 GDI следуют документированным контрактам
        // (паттерн crates/shell/src/platform/screen_capture.rs): каждый handle
        // проверяется, все ресурсы освобождаются до выхода.
        unsafe {
            let mut rect = Rect { left: 0, top: 0, right: 0, bottom: 0 };
            if GetClientRect(hwnd, &raw mut rect) == 0 {
                return None;
            }
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            if w < 32 || h < 32 {
                return None;
            }

            let win_dc = GetDC(hwnd);
            if win_dc.is_null() {
                return None;
            }
            let mem_dc = CreateCompatibleDC(win_dc);
            if mem_dc.is_null() {
                ReleaseDC(hwnd, win_dc);
                return None;
            }
            let bitmap = CreateCompatibleBitmap(win_dc, w, h);
            if bitmap.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(hwnd, win_dc);
                return None;
            }
            let old_obj = SelectObject(mem_dc, bitmap);
            let printed = PrintWindow(hwnd, mem_dc, PW_FLAGS);

            let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
            let got_bits = if printed != 0 {
                let mut bmi = BitmapInfo {
                    bmi_header: BitmapInfoHeader {
                        bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                        bi_width: w,
                        // Отрицательная высота → top-down DIB (строка 0 сверху).
                        bi_height: -h,
                        bi_planes: 1,
                        bi_bit_count: 32,
                        bi_compression: BI_RGB,
                        bi_size_image: 0,
                        bi_x_pels_per_meter: 0,
                        bi_y_pels_per_meter: 0,
                        bi_clr_used: 0,
                        bi_clr_important: 0,
                    },
                    bmi_colors: [0],
                };
                GetDIBits(
                    mem_dc,
                    bitmap,
                    0,
                    h as u32,
                    pixels.as_mut_ptr().cast::<c_void>(),
                    &raw mut bmi,
                    DIB_RGB_COLORS,
                ) > 0
            } else {
                false
            };

            SelectObject(mem_dc, old_obj);
            DeleteObject(bitmap);
            DeleteDC(mem_dc);
            ReleaseDC(hwnd, win_dc);

            if !got_bits {
                return None;
            }

            // Средний цвет центрального блока 32×32; GDI отдаёт BGRA.
            let (cx, cy) = (w / 2, h / 2);
            let mut sum = [0u64; 3];
            let mut count = 0u64;
            for y in (cy - 16)..(cy + 16) {
                for x in (cx - 16)..(cx + 16) {
                    let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
                    sum[0] += u64::from(pixels[off + 2]); // R
                    sum[1] += u64::from(pixels[off + 1]); // G
                    sum[2] += u64::from(pixels[off]); // B
                    count += 1;
                }
            }
            if count == 0 {
                return None;
            }
            Some([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
            ])
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_color_matches_itself_linear() {
        let avg = [
            expected_byte(PROBE_COLOR.r, false),
            expected_byte(PROBE_COLOR.g, false),
            expected_byte(PROBE_COLOR.b, false),
        ];
        assert_eq!(classify(avg, false), Signal::Match);
    }

    #[test]
    fn probe_color_matches_itself_srgb() {
        let avg = [
            expected_byte(PROBE_COLOR.r, true),
            expected_byte(PROBE_COLOR.g, true),
            expected_byte(PROBE_COLOR.b, true),
        ];
        assert_eq!(classify(avg, true), Signal::Match);
    }

    #[test]
    fn white_is_detected_as_white() {
        assert_eq!(classify([255, 255, 255], false), Signal::White);
        assert_eq!(classify([243, 246, 250], false), Signal::White);
    }

    #[test]
    fn black_is_other_not_white() {
        assert_eq!(classify([0, 0, 0], false), Signal::Other([0, 0, 0]));
    }

    #[test]
    fn swapped_channels_do_not_match() {
        // Пробный цвет с перепутанными R и B не должен давать Match:
        // каналы выбраны попарно различными (64/140/217, дельта > TOLERANCE).
        let swapped = [
            expected_byte(PROBE_COLOR.b, false),
            expected_byte(PROBE_COLOR.g, false),
            expected_byte(PROBE_COLOR.r, false),
        ];
        assert_eq!(classify(swapped, false), Signal::Other(swapped));
    }

    #[test]
    fn expected_byte_srgb_encodes() {
        // 0.25 линейно ≈ 137 в sRGB-байтах, 64 без кодирования.
        assert_eq!(expected_byte(0.25, false), 64);
        let srgb = expected_byte(0.25, true);
        assert!((130..=143).contains(&srgb), "sRGB(0.25) ≈ 137, получили {srgb}");
    }

    const CANDIDATES: [(wgpu::Backends, &str); 3] = [
        (wgpu::Backends::VULKAN, "Vulkan"),
        (wgpu::Backends::GL, "GL"),
        (wgpu::Backends::DX12, "DX12"),
    ];

    #[test]
    fn reorder_by_cache_no_cache_keeps_default_order() {
        let order = reorder_by_cache(CANDIDATES, None);
        assert_eq!(order, CANDIDATES.to_vec());
    }

    #[test]
    fn reorder_by_cache_moves_cached_candidate_first() {
        let order = reorder_by_cache(CANDIDATES, Some(wgpu::Backends::DX12));
        assert_eq!(
            order,
            vec![
                (wgpu::Backends::DX12, "DX12"),
                (wgpu::Backends::VULKAN, "Vulkan"),
                (wgpu::Backends::GL, "GL"),
            ]
        );
    }

    #[test]
    fn reorder_by_cache_already_first_is_noop() {
        let order = reorder_by_cache(CANDIDATES, Some(wgpu::Backends::VULKAN));
        assert_eq!(order, CANDIDATES.to_vec());
    }

    #[test]
    fn reorder_by_cache_unknown_backend_keeps_default_order() {
        // Не одно из трёх известных значений (например, PRIMARY) — не находится
        // в `candidates`, порядок не меняется.
        let order = reorder_by_cache(CANDIDATES, Some(wgpu::Backends::PRIMARY));
        assert_eq!(order, CANDIDATES.to_vec());
    }

    #[test]
    fn cache_roundtrip_writes_and_reads_back() {
        // cache_path() резолвится от current_exe() — при `cargo test` это тестовый
        // бинарник в target/…/deps/, поэтому запись реально изолирована per-run
        // (не пересекается с production `lumen.exe`).
        let path = cache_path();
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_cache(), None);
        let entry = ProbeCache {
            winner: "DX12".to_string(),
            adapter: "Intel(R) Iris(R) Plus Graphics".to_string(),
            driver: "Intel Corporation 31.0.101.2114".to_string(),
            app: env!("CARGO_PKG_VERSION").to_string(),
        };
        write_cache(&entry);
        assert_eq!(read_cache(), Some(entry));
        let _ = std::fs::remove_file(&path);
    }

    /// BUG-405 срез 14: кэш формата v1 (одно слово) больше не даёт права
    /// пропускать кандидатов — иначе однажды принятый DX12 навсегда закрывает
    /// дорогу Vulkan'у, который вдвое дешевле по кадру прокрутки.
    #[test]
    fn legacy_one_word_cache_is_ignored() {
        assert_eq!(parse_cache("DX12"), None);
        assert_eq!(parse_cache("DX12\n"), None);
        assert_eq!(parse_cache(""), None);
    }

    #[test]
    fn cache_without_environment_key_is_ignored() {
        // Победитель без ключа окружения — те же «улики без основания».
        assert_eq!(parse_cache("winner=DX12\n"), None);
        assert_eq!(parse_cache("winner=DX12\nadapter=Intel\ndriver=x\n"), None);
    }

    #[test]
    fn cache_with_unknown_winner_is_ignored() {
        assert_eq!(
            parse_cache("winner=Metal\nadapter=Intel\ndriver=x\napp=0.5.0\n"),
            None
        );
    }

    #[test]
    fn cache_parses_full_v2_entry() {
        let text = "winner=Vulkan\nadapter=Intel(R) Iris(R) Plus Graphics\n\
                    driver=Intel Corporation 31.0.101.2114\napp=0.5.0\n";
        let cache = parse_cache(text).expect("v2-запись разбирается");
        assert_eq!(cache.winner, "Vulkan");
        assert_eq!(cache.adapter, "Intel(R) Iris(R) Plus Graphics");
        assert_eq!(cache.driver, "Intel Corporation 31.0.101.2114");
        assert_eq!(cache.app, "0.5.0");
        assert_eq!(parse_cache(&serialize_cache(&cache)), Some(cache));
    }

    fn rejected(backends: wgpu::Backends, name: &'static str, texture: Signal) -> Rejected {
        Rejected {
            backends,
            name,
            texture,
            adapter: Some(("Intel(R) UHD Graphics".into(), "Intel Corporation x".into())),
        }
    }

    fn cache_for(winner: &str) -> ProbeCache {
        ProbeCache {
            winner: winner.into(),
            adapter: "Intel(R) UHD Graphics".into(),
            driver: "Intel Corporation x".into(),
            app: env!("CARGO_PKG_VERSION").into(),
        }
    }

    /// BUG-1073: все кандидаты отклонены по захвату, но readback рабочий —
    /// берём первый с `texture=ok`, а не статическую цепочку.
    #[test]
    fn fallback_choice_takes_first_texture_ok() {
        let all = [
            rejected(wgpu::Backends::GL, "GL", Signal::Unavailable),
            rejected(wgpu::Backends::VULKAN, "Vulkan", Signal::Match),
            rejected(wgpu::Backends::DX12, "DX12", Signal::Match),
        ];
        assert_eq!(fallback_choice(&all, None).map(|r| r.name), Some("Vulkan"));
    }

    /// Кэш поручился за DX12 — берём его, хотя Vulkan раньше в порядке.
    #[test]
    fn fallback_choice_prefers_vouched() {
        let all = [
            rejected(wgpu::Backends::VULKAN, "Vulkan", Signal::Match),
            rejected(wgpu::Backends::GL, "GL", Signal::Unavailable),
            rejected(wgpu::Backends::DX12, "DX12", Signal::Match),
        ];
        let cache = cache_for("DX12");
        assert_eq!(fallback_choice(&all, Some(&cache)).map(|r| r.name), Some("DX12"));
    }

    /// Без подтверждённого readback выбор не делается — статическая цепочка.
    #[test]
    fn fallback_choice_none_without_texture_ok() {
        let all = [
            rejected(wgpu::Backends::VULKAN, "Vulkan", Signal::White),
            rejected(wgpu::Backends::GL, "GL", Signal::Unavailable),
            rejected(wgpu::Backends::DX12, "DX12", Signal::Other([0, 0, 0])),
        ];
        assert!(fallback_choice(&all, Some(&cache_for("Vulkan"))).is_none());
        assert!(fallback_choice(&[], None).is_none());
    }

    /// Бюджет обрывает пробу только при поручительстве кэша: тот же
    /// кандидат, адаптер, драйвер и версия, readback подтверждён. Машина с
    /// BUG-275 (кэш — DX12) после отклонённого Vulkan пробует дальше.
    #[test]
    fn vouched_by_cache_requires_full_match() {
        let vk = [rejected(wgpu::Backends::VULKAN, "Vulkan", Signal::Match)];
        assert!(vouched_by_cache(&vk, Some(&cache_for("Vulkan"))).is_some());
        assert!(vouched_by_cache(&vk, None).is_none());
        assert!(vouched_by_cache(&vk, Some(&cache_for("DX12"))).is_none());
        let mut other_driver = cache_for("Vulkan");
        other_driver.driver = "Intel Corporation y".into();
        assert!(vouched_by_cache(&vk, Some(&other_driver)).is_none());
        let mut old_app = cache_for("Vulkan");
        old_app.app = "0.0.1".into();
        assert!(vouched_by_cache(&vk, Some(&old_app)).is_none());
        let white = [rejected(wgpu::Backends::VULKAN, "Vulkan", Signal::White)];
        assert!(vouched_by_cache(&white, Some(&cache_for("Vulkan"))).is_none());
        let unopened = [Rejected {
            backends: wgpu::Backends::VULKAN,
            name: "Vulkan",
            texture: Signal::Match,
            adapter: None,
        }];
        assert!(vouched_by_cache(&unopened, Some(&cache_for("Vulkan"))).is_none());
    }

    /// Правило приёма не изменилось: `present=WHITE` при `texture=ok`
    /// кандидата не принимает (это случай BUG-275) — он лишь резерв.
    #[test]
    fn acceptance_rule() {
        assert!(is_accepted(Signal::Match, Signal::Unavailable));
        assert!(is_accepted(Signal::Unavailable, Signal::Match));
        assert!(!is_accepted(Signal::White, Signal::Match));
        assert!(!is_accepted(Signal::Unavailable, Signal::Unavailable));
    }

    #[test]
    fn candidates_before_lists_higher_priority_only() {
        assert_eq!(
            candidates_before(CANDIDATES, wgpu::Backends::DX12),
            vec![(wgpu::Backends::VULKAN, "Vulkan"), (wgpu::Backends::GL, "GL")]
        );
        assert_eq!(
            candidates_before(CANDIDATES, wgpu::Backends::VULKAN),
            Vec::new()
        );
    }
}
