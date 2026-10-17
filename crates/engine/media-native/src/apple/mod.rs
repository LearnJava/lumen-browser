//! macOS: H.264 / HEVC из VideoToolbox и AAC из AudioToolbox (COMPAT-MEDIA-OSDEC-MAC,
//! ADR-035 п.3). Кодеки патентованные, поэтому берутся только у ОС и нигде не вшиваются.
//!
//! Пакеты отдаёт наш демуксер ([`crate::session`]). Видео: `VTDecompressionSession`
//! над форматом `avcC`/`hvcC` (пакеты MP4 уже с префиксами длины — VideoToolbox
//! принимает их как есть, в Annex B переводить не нужно); VideoToolbox сам берёт
//! аппаратный декодер (Apple Silicon, Intel с Quick Sync), а без него — программный.
//! Звук: `AudioConverter` AAC → PCM i16.
//!
//! Все функции — C-интерфейсы фреймворков, объявленные вручную ([`ffi`]);
//! фреймворки есть в каждой macOS, поэтому, в отличие от Windows N/KN, динамическая
//! загрузка не нужна.
//!
//! **Наличие.** [`availability`] один раз спрашивает ОС; реестр по нему отвечает на
//! `canPlayType`. H.264 и AAC есть в каждой macOS, HEVC — аппаратно (Apple Silicon,
//! Intel с поддержкой) или программно с macOS 11.

mod audio;
mod ffi;
mod video;

use std::ffi::{c_void, CString};
use std::sync::OnceLock;

use lumen_media_demux::{Codec, TrackInfo};

use crate::profile::{aac_object_supported, h264_profile_supported, hevc_profile_supported};
use crate::registry::{AudioBackend, AudioPcmDecoder, VideoBackend, VideoFrameDecoder};
use ffi::{CFTypeRef, OSStatus};

/// Переменная окружения: `0` отключает декодеры ОС (отладка отката на встроенные
/// кодеки); `sw` оставляет их, но не просит аппаратный декодер.
pub(crate) const DISABLE_ENV: &str = "LUMEN_OS_DECODERS";

/// `LUMEN_OS_DECODERS=sw`: не использовать аппаратное декодирование.
pub(crate) fn software_only() -> bool {
    static SW: OnceLock<bool> = OnceLock::new();
    *SW.get_or_init(|| std::env::var(DISABLE_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("sw")))
}

/// Объект CoreFoundation, которым мы владеем (`CFRelease` при уничтожении).
pub(crate) struct Cf(CFTypeRef);

impl Cf {
    /// Взять владение. `None` для `NULL` (функция не смогла создать объект).
    pub(crate) fn new(raw: CFTypeRef) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw))
    }

    pub(crate) fn raw(&self) -> CFTypeRef {
        self.0
    }

    /// Строка CF из литерала.
    pub(crate) fn string(s: &str) -> Option<Self> {
        let c = CString::new(s).ok()?;
        // SAFETY: `c` — NUL-терминированная строка, живёт до конца вызова; функция копирует её.
        Self::new(unsafe { ffi::CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), ffi::kCFStringEncodingUTF8) })
    }

    /// Данные CF — копия `bytes`.
    pub(crate) fn data(bytes: &[u8]) -> Option<Self> {
        let len = isize::try_from(bytes.len()).ok()?;
        // SAFETY: `bytes` валиден на `len` байт; функция копирует содержимое.
        Self::new(unsafe { ffi::CFDataCreate(std::ptr::null(), bytes.as_ptr(), len) })
    }

    /// 32-битное целое CF.
    pub(crate) fn number(n: i32) -> Option<Self> {
        // SAFETY: указатель на `n` валиден на время вызова; функция копирует значение.
        Self::new(unsafe {
            ffi::CFNumberCreate(std::ptr::null(), ffi::kCFNumberSInt32Type, (&raw const n).cast::<c_void>())
        })
    }

    /// Словарь CF из пар «ключ — значение» (объекты CF; словарь удерживает их сам).
    pub(crate) fn dictionary(pairs: &[(CFTypeRef, CFTypeRef)]) -> Option<Self> {
        let keys: Vec<*const c_void> = pairs.iter().map(|p| p.0).collect();
        let values: Vec<*const c_void> = pairs.iter().map(|p| p.1).collect();
        let count = isize::try_from(pairs.len()).ok()?;
        // SAFETY: массивы длиной `count`, ключи и значения — живые объекты CF;
        // стандартные наборы обратных вызовов удерживают их на время жизни словаря.
        Self::new(unsafe {
            ffi::CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                count,
                &raw const ffi::kCFTypeDictionaryKeyCallBacks,
                &raw const ffi::kCFTypeDictionaryValueCallBacks,
            )
        })
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: владеем одной ссылкой на объект, полученную от функции `Create`.
        unsafe { ffi::CFRelease(self.0) };
    }
}

/// `OSStatus` с именем вызова — для сообщений об ошибках.
pub(crate) fn check(what: &str, status: OSStatus) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("{what}: OSStatus {status}")) }
}

/// Какие кодеки у ОС есть.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Availability {
    pub h264: bool,
    pub hevc: bool,
    pub aac: bool,
}

/// Старшая версия ядра Darwin (20 = macOS 11); `None`, если sysctl не ответил.
fn darwin_major() -> Option<u32> {
    let name = CString::new("kern.osrelease").ok()?;
    let mut buf = [0u8; 32];
    let mut len = buf.len();
    // SAFETY: `buf` вмещает `len` байт, имя — NUL-терминированная строка.
    let rc = unsafe {
        ffi::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast::<c_void>(), &raw mut len, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return None;
    }
    let text = std::str::from_utf8(buf.get(..len)?).ok()?;
    text.split('.').next()?.trim_end_matches('\0').parse().ok()
}

/// Есть ли у ОС декодеры (один опрос на процесс).
pub(crate) fn availability() -> Availability {
    static CACHE: OnceLock<Availability> = OnceLock::new();
    *CACHE.get_or_init(|| {
        if std::env::var(DISABLE_ENV).is_ok_and(|v| v == "0") {
            return Availability { h264: false, hevc: false, aac: false };
        }
        // SAFETY: запрос возможностей аппаратного декодера; указателей не принимает.
        let hevc_hw = unsafe { ffi::VTIsHardwareDecodeSupported(ffi::kCMVideoCodecType_HEVC) } != 0;
        Availability {
            h264: true,
            // Программный HEVC в VideoToolbox — с macOS 11.
            hevc: hevc_hw || darwin_major().is_some_and(|m| m >= 20),
            aac: audio::converter_available(),
        }
    })
}

/// Видеобэкенд VideoToolbox.
pub(crate) struct VtVideoBackend;

impl VideoBackend for VtVideoBackend {
    fn name(&self) -> &'static str {
        "videotoolbox"
    }

    fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        match codec {
            Codec::H264 => availability().h264 && h264_profile_supported(codec_string),
            Codec::Hevc => availability().hevc && hevc_profile_supported(codec_string),
            _ => false,
        }
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn VideoFrameDecoder>, String> {
        Ok(Box::new(video::VtVideoDecoder::new(track)?))
    }
}

/// Аудиобэкенд AudioToolbox.
pub(crate) struct AtAudioBackend;

impl AudioBackend for AtAudioBackend {
    fn name(&self) -> &'static str {
        "audiotoolbox"
    }

    fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        codec == Codec::Aac && availability().aac && aac_object_supported(codec_string)
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn AudioPcmDecoder>, String> {
        Ok(Box::new(audio::AacDecoder::new(track)?))
    }
}
