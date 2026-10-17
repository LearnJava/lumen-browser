//! Linux: H.264 / HEVC / AAC из GStreamer (COMPAT-MEDIA-OSDEC-LINUX, ADR-035 п.3).
//! Кодеки патентованные, поэтому берутся только у системы и нигде не вшиваются.
//!
//! **Почему GStreamer.** Задача оставляла выбор: напрямую VA-API (libva) плюс
//! системный `libavcodec` для программного пути, либо GStreamer. Выбран GStreamer:
//!
//! - его ABI стабилен в пределах 1.x, а `AVCodecContext`/`AVFrame` у FFmpeg
//!   меняют раскладку между мажорными версиями (живой баг со смещением `pts` —
//!   `subsystems/media-ffmpeg.md`), и вызывать системный `libavcodec` вслепую —
//!   значит гадать смещения полей;
//! - аппаратный путь VA-API получается сам: `decodebin` берёт декодер по рангу, и
//!   плагин `va` (`vah264dec`, `vah265dec`; в старых версиях — `vaapih264dec`)
//!   стоит выше программного `avdec_h264`. Свой разбор заголовков слайсов H.264/HEVC
//!   и управление буфером опорных кадров поверх libva не пишем: там ошибка
//!   означает падение драйвера, а проверить без железа нельзя;
//! - программный запасной путь — `gst-libav` дистрибутива, подгружаемый при запуске;
//!   в бинарник и в поставку не входит ничего.
//!
//! Библиотеки GStreamer находятся через `dlopen` ([`ffi`]) — сборка и запуск не
//! зависят от их наличия. Пакеты отдаёт наш демуксер ([`crate::session`]); в
//! GStreamer они идут через `appsrc` (формат `avc`/`hvc1`/`raw` без перепаковки),
//! результат — RGBA ([`video`]) и PCM i16 ([`audio`]) из `appsink`.
//!
//! **Наличие.** [`availability`] один раз находит GStreamer и спрашивает у его
//! реестра, есть ли декодеры H.264/HEVC/AAC; по нему реестр кодеков отвечает на
//! `canPlayType`. Нет GStreamer или кодеков — `canPlayType('video/mp4; codecs=avc1…')`
//! возвращает `""`, а шелл показывает плашку с командой для дистрибутива
//! ([`crate::distro`]).

mod audio;
mod ffi;
mod pipeline;
mod video;

use std::ffi::{c_char, CStr, CString};
use std::sync::OnceLock;

use lumen_media_demux::{Codec, TrackInfo};

use crate::gst_caps::{AAC_PROBE_CAPS, H264_PROBE_CAPS, HEVC_PROBE_CAPS};
use crate::profile::{aac_object_supported, h264_profile_supported, hevc_profile_supported};
use crate::registry::{AudioBackend, AudioPcmDecoder, VideoBackend, VideoFrameDecoder};
use ffi::{api, Api, Obj, FACTORY_TYPE_DECODER, RANK_MARGINAL};

/// Переменная окружения: `0` отключает декодеры ОС (отладка отката на встроенные
/// кодеки); `sw` оставляет их, но убирает аппаратные декодеры из выбора.
pub(crate) const DISABLE_ENV: &str = "LUMEN_OS_DECODERS";

/// Какие кодеки у системы есть.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Availability {
    pub h264: bool,
    pub hevc: bool,
    pub aac: bool,
    /// Среди декодеров H.264 есть аппаратный (VA-API, NVDEC, V4L2…): только для журнала и тестов.
    pub hardware_h264: bool,
}

const NONE: Availability = Availability { h264: false, hevc: false, aac: false, hardware_h264: false };

/// Элементы GStreamer, без которых конвейер не построить (`appsrc`/`appsink` —
/// плагин `app` из gst-plugins-base, как и `videoconvert`, `audioconvert`).
const REQUIRED_ELEMENTS: [&CStr; 6] =
    [c"appsrc", c"appsink", c"decodebin", c"videoconvert", c"audioconvert", c"audioresample"];

/// Есть ли у системы декодеры (один опрос на процесс; он же инициализирует GStreamer).
pub(crate) fn availability() -> Availability {
    static CACHE: OnceLock<Availability> = OnceLock::new();
    *CACHE.get_or_init(probe)
}

/// Имя элемента похоже на аппаратный декодер: VA-API (`va…`, `vaapi…`), NVDEC
/// (`nv…`), V4L2 (`v4l2…`), Intel Media SDK / oneVPL (`msdk…`, `qsv…`).
fn looks_hardware(factory_name: &str) -> bool {
    ["va", "nv", "v4l2", "msdk", "qsv"].iter().any(|prefix| factory_name.starts_with(prefix))
}

fn probe() -> Availability {
    let off = std::env::var(DISABLE_ENV).ok();
    if off.as_deref() == Some("0") {
        return NONE;
    }
    let Ok(api) = api() else { return NONE };
    let all_present = REQUIRED_ELEMENTS.iter().all(|name| {
        // SAFETY: имя — NUL-терминированный литерал.
        let factory = unsafe { (api.gst_element_factory_find)(name.as_ptr()) };
        if factory.is_null() {
            return false;
        }
        // SAFETY: освобождаем ссылку, которую дал `find`.
        unsafe { (api.gst_object_unref)(factory) };
        true
    });
    if !all_present {
        return NONE;
    }
    let software_only = off.as_deref().is_some_and(|v| v.eq_ignore_ascii_case("sw"));
    let h264 = scan_decoders(api, H264_PROBE_CAPS, software_only);
    let hevc = scan_decoders(api, HEVC_PROBE_CAPS, software_only);
    let aac = scan_decoders(api, AAC_PROBE_CAPS, software_only);
    Availability {
        h264: h264.any,
        hevc: hevc.any,
        aac: aac.any,
        hardware_h264: h264.hardware,
    }
}

/// Результат просмотра декодеров под один кодек.
struct Scan {
    any: bool,
    hardware: bool,
}

/// Просмотреть декодеры реестра GStreamer, принимающие `caps_text`. При
/// `lower_hardware_rank` аппаратные декодеры получают ранг 0 — `decodebin` их
/// больше не выберет (`LUMEN_OS_DECODERS=sw`).
fn scan_decoders(api: &Api, caps_text: &str, lower_hardware_rank: bool) -> Scan {
    let mut scan = Scan { any: false, hardware: false };
    let Ok(text) = CString::new(caps_text) else { return scan };
    // SAFETY: строка NUL-терминирована.
    let caps = unsafe { (api.gst_caps_from_string)(text.as_ptr()) };
    if caps.is_null() {
        return scan;
    }
    // SAFETY: список выдаёт GStreamer, освобождаем его ниже `gst_plugin_feature_list_free`.
    let list = unsafe { (api.gst_element_factory_list_get_elements)(FACTORY_TYPE_DECODER, RANK_MARGINAL) };
    let mut node = list;
    while !node.is_null() {
        // SAFETY: `node` — звено списка GLib: `data` — `GstElementFactory*`, `next` — следующее звено.
        let (factory, next) = unsafe { ((*node).data as Obj, (*node).next) };
        node = next;
        // SAFETY: фабрика жива, пока жив список; `caps` — наш объект.
        if unsafe { (api.gst_element_factory_can_sink_any_caps)(factory, caps) } == 0 {
            continue;
        }
        let name = factory_name(api, factory);
        let hardware = looks_hardware(&name);
        if hardware && lower_hardware_rank {
            // SAFETY: фабрика — `GstPluginFeature`; ранг 0 = `GST_RANK_NONE`.
            unsafe { (api.gst_plugin_feature_set_rank)(factory, 0) };
            continue;
        }
        scan.any = true;
        scan.hardware |= hardware;
    }
    if !list.is_null() {
        // SAFETY: список получен выше и больше не используется.
        unsafe { (api.gst_plugin_feature_list_free)(list) };
    }
    // SAFETY: освобождаем свои caps.
    unsafe { (api.gst_mini_object_unref)(caps) };
    scan
}

fn factory_name(api: &Api, factory: Obj) -> String {
    // SAFETY: имя принадлежит фабрике и живёт вместе с ней; копируем сразу.
    let p: *const c_char = unsafe { (api.gst_plugin_feature_get_name)(factory) };
    if p.is_null() {
        return String::new();
    }
    // SAFETY: непустой указатель — NUL-терминированная строка.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Видеобэкенд GStreamer.
pub(crate) struct GstVideoBackend;

impl VideoBackend for GstVideoBackend {
    fn name(&self) -> &'static str {
        "gstreamer"
    }

    fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        match codec {
            Codec::H264 => availability().h264 && h264_profile_supported(codec_string),
            Codec::Hevc => availability().hevc && hevc_profile_supported(codec_string),
            _ => false,
        }
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn VideoFrameDecoder>, String> {
        Ok(Box::new(video::GstVideoDecoder::new(track)?))
    }
}

/// Аудиобэкенд GStreamer.
pub(crate) struct GstAudioBackend;

impl AudioBackend for GstAudioBackend {
    fn name(&self) -> &'static str {
        "gstreamer"
    }

    fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        codec == Codec::Aac && availability().aac && aac_object_supported(codec_string)
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn AudioPcmDecoder>, String> {
        Ok(Box::new(audio::GstAacDecoder::new(track)?))
    }
}
