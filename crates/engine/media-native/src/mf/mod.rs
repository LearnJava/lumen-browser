//! Windows: H.264 / HEVC / AAC из Media Foundation (COMPAT-MEDIA-OSDEC-WIN,
//! ADR-035 п.3). Кодеки патентованные, поэтому берутся только у ОС и нигде не
//! вшиваются.
//!
//! Декодеры — готовые MFT ОС (`IMFTransform`), которым пакеты отдаёт наш же
//! демуксер ([`crate::session`]); `IMFSourceReader` не нужен: контейнер уже
//! разобран, а общий курсор демуксера держит видео и звук синхронными.
//!
//! **Загрузка.** Функции `mfplat.dll` (`MFStartup`, `MFTEnumEx`, `MFCreateSample`…)
//! в exe не линкуются: их берёт [`Api::get`] через `LoadLibraryExW`. Статическая
//! зависимость не дала бы запуститься на Windows N/KN без Media Feature Pack,
//! где этой DLL нет вовсе. Интерфейсы (`IMFTransform` и др.) — вызовы по vtable,
//! к импортам не относятся.
//!
//! **Наличие.** [`availability`] один раз спрашивает ОС, есть ли MFT для H.264,
//! HEVC и AAC; реестр по нему отвечает на `canPlayType`, а оболочка — какую
//! подсказку показать ([`crate::os_codec_status`]).

mod audio;
mod blocklist;
mod d3d;
mod video;

use std::ffi::c_void;
use std::sync::OnceLock;

use lumen_media_demux::{Codec, TrackInfo};
use windows::core::{s, w, Interface, GUID, HRESULT, PCSTR};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaType, IMFSample,
    IMFVideoSampleAllocatorEx, MFAudioFormat_AAC, MFMediaType_Audio,
    MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_HEVC, MFSTARTUP_LITE, MFT_CATEGORY_AUDIO_DECODER,
    MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_REGISTER_TYPE_INFO,
    MF_VERSION,
};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

use crate::profile::{aac_object_supported, h264_profile_supported, hevc_profile_supported};
use crate::registry::{AudioBackend, AudioPcmDecoder, VideoBackend, VideoFrameDecoder};

/// Переменная окружения: `0` отключает декодеры ОС (поведение Windows N/KN без
/// Media Feature Pack, отладка отката на встроенные кодеки); `sw` оставляет их,
/// но заставляет видео идти только через программный MFT, без D3D11VA.
pub(crate) const DISABLE_ENV: &str = "LUMEN_OS_DECODERS";

/// `LUMEN_OS_DECODERS=sw`: не использовать аппаратное декодирование (D3D11VA).
/// Читается один раз, как и [`DISABLE_ENV`].
pub(crate) fn software_only() -> bool {
    static SW: OnceLock<bool> = OnceLock::new();
    *SW.get_or_init(|| std::env::var(DISABLE_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("sw")))
}

type MfStartupFn = unsafe extern "system" fn(u32, u32) -> HRESULT;
type MftEnumExFn = unsafe extern "system" fn(
    GUID,
    u32,
    *const MFT_REGISTER_TYPE_INFO,
    *const MFT_REGISTER_TYPE_INFO,
    *mut *mut *mut c_void,
    *mut u32,
) -> HRESULT;
type MfCreateObjFn = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type MfCreateBufferFn = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
type MfCreateAttributesFn = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type MfCreateDxgiManagerFn = unsafe extern "system" fn(*mut u32, *mut *mut c_void) -> HRESULT;
type MfCreateAllocatorExFn = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> HRESULT;

/// Адрес экспорта `name` библиотеки `lib` как указателя на функцию типа `F`.
///
/// # Safety
/// `F` должен быть `extern "system" fn(..)` с сигнатурой, совпадающей с экспортом.
unsafe fn symbol<F: Copy>(lib: HMODULE, name: PCSTR) -> Option<F> {
    debug_assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<usize>());
    // SAFETY: `lib` — живой модуль (не выгружается); по контракту `F` — указатель
    // на функцию того же размера, что и `address`.
    unsafe {
        let address = GetProcAddress(lib, name)?;
        Some(std::mem::transmute_copy::<_, F>(&address))
    }
}

/// Функции `mfplat.dll`, найденные во время работы.
pub(crate) struct Api {
    enum_ex: MftEnumExFn,
    create_media_type: MfCreateObjFn,
    create_sample: MfCreateObjFn,
    create_memory_buffer: MfCreateBufferFn,
    create_attributes: MfCreateAttributesFn,
    /// Нужны только D3D11VA: если их нет, видео идёт программным путём.
    create_dxgi_manager: Option<MfCreateDxgiManagerFn>,
    /// `MFCreateVideoSampleAllocatorEx` живёт в `evr.dll`.
    create_allocator_ex: Option<MfCreateAllocatorExFn>,
}

impl Api {
    /// `None` — Media Foundation на этой машине нет (или она отключена
    /// [`DISABLE_ENV`]).
    pub(crate) fn get() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(Self::load).as_ref()
    }

    fn load() -> Option<Api> {
        if std::env::var(DISABLE_ENV).is_ok_and(|v| v == "0") {
            return None;
        }
        // SAFETY: имена и сигнатуры функций — из заголовков `mfapi.h`; DLL ищется
        // только в System32 (не в каталоге exe и не по PATH).
        unsafe {
            let lib = LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            let start: MfStartupFn = symbol(lib, s!("MFStartup"))?;
            let api = Api {
                enum_ex: symbol(lib, s!("MFTEnumEx"))?,
                create_media_type: symbol(lib, s!("MFCreateMediaType"))?,
                create_sample: symbol(lib, s!("MFCreateSample"))?,
                create_memory_buffer: symbol(lib, s!("MFCreateMemoryBuffer"))?,
                create_attributes: symbol(lib, s!("MFCreateAttributes"))?,
                create_dxgi_manager: symbol(lib, s!("MFCreateDXGIDeviceManager")),
                create_allocator_ex: LoadLibraryExW(w!("evr.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
                    .ok()
                    .and_then(|evr| symbol(evr, s!("MFCreateVideoSampleAllocatorEx"))),
            };
            // Один раз на процесс, `MFShutdown` не нужен: библиотека живёт до выхода.
            start(MF_VERSION, MFSTARTUP_LITE).is_ok().then_some(api)
        }
    }

    pub(crate) fn new_media_type(&self) -> Result<IMFMediaType, String> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: `raw` — выходной параметр; при успехе в нём владеющий указатель.
        unsafe {
            (self.create_media_type)(&mut raw).ok().map_err(|e| format!("MFCreateMediaType: {e}"))?;
            Ok(IMFMediaType::from_raw(raw))
        }
    }

    pub(crate) fn new_sample(&self) -> Result<IMFSample, String> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: как в `new_media_type`.
        unsafe {
            (self.create_sample)(&mut raw).ok().map_err(|e| format!("MFCreateSample: {e}"))?;
            Ok(IMFSample::from_raw(raw))
        }
    }

    pub(crate) fn new_buffer(&self, len: u32) -> Result<IMFMediaBuffer, String> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: как в `new_media_type`.
        unsafe {
            (self.create_memory_buffer)(len, &mut raw).ok().map_err(|e| format!("MFCreateMemoryBuffer: {e}"))?;
            Ok(IMFMediaBuffer::from_raw(raw))
        }
    }

    pub(crate) fn new_attributes(&self, capacity: u32) -> Result<IMFAttributes, String> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: как в `new_media_type`.
        unsafe {
            (self.create_attributes)(&mut raw, capacity).ok().map_err(|e| format!("MFCreateAttributes: {e}"))?;
            Ok(IMFAttributes::from_raw(raw))
        }
    }

    /// Менеджер устройств DXGI и токен, которым ему передаётся устройство.
    pub(crate) fn new_dxgi_manager(&self) -> Result<(IMFDXGIDeviceManager, u32), String> {
        let create = self.create_dxgi_manager.ok_or("MFCreateDXGIDeviceManager недоступна")?;
        let (mut raw, mut token) = (std::ptr::null_mut(), 0u32);
        // SAFETY: как в `new_media_type`.
        unsafe {
            create(&mut token, &mut raw).ok().map_err(|e| format!("MFCreateDXGIDeviceManager: {e}"))?;
            Ok((IMFDXGIDeviceManager::from_raw(raw), token))
        }
    }

    pub(crate) fn new_sample_allocator(&self) -> Result<IMFVideoSampleAllocatorEx, String> {
        let create = self.create_allocator_ex.ok_or("MFCreateVideoSampleAllocatorEx недоступна")?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: `raw` — выходной параметр с владеющим указателем нужного интерфейса.
        unsafe {
            create(&IMFVideoSampleAllocatorEx::IID, &mut raw)
                .ok()
                .map_err(|e| format!("MFCreateVideoSampleAllocatorEx: {e}"))?;
            Ok(IMFVideoSampleAllocatorEx::from_raw(raw))
        }
    }

    /// MFT-декодеры категории `category` для входного формата `input`.
    /// Только синхронные программные/DXVA-MFT ОС: у асинхронных аппаратных
    /// (`MFT_ENUM_FLAG_HARDWARE`) другая модель событий.
    fn decoders(&self, category: GUID, major: GUID, subtype: GUID) -> Vec<IMFActivate> {
        let input = MFT_REGISTER_TYPE_INFO { guidMajorType: major, guidSubtype: subtype };
        let flags = (MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0) as u32;
        let mut array: *mut *mut c_void = std::ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: массив размером `count` выделен `CoTaskMemAlloc` самой функцией;
        // каждый элемент — владеющий указатель `IMFActivate`, сам массив освобождаем.
        unsafe {
            if (self.enum_ex)(category, flags, &input, std::ptr::null(), &mut array, &mut count).is_err()
                || array.is_null()
            {
                return Vec::new();
            }
            let list = (0..count as usize)
                .filter_map(|i| {
                    let p = *array.add(i);
                    (!p.is_null()).then(|| IMFActivate::from_raw(p))
                })
                .collect();
            CoTaskMemFree(Some(array as *const c_void));
            list
        }
    }

    /// Первый подходящий MFT, готовый к работе.
    pub(crate) fn create_transform<T: Interface>(
        &self,
        category: GUID,
        major: GUID,
        subtype: GUID,
    ) -> Result<T, String> {
        let mut last = "декодер Media Foundation не найден".to_string();
        for activate in self.decoders(category, major, subtype) {
            // SAFETY: активатор получен от `MFTEnumEx` выше.
            match unsafe { activate.ActivateObject::<T>() } {
                Ok(t) => return Ok(t),
                Err(e) => last = format!("ActivateObject: {e}"),
            }
        }
        Err(last)
    }

    fn has_decoder(&self, category: GUID, major: GUID, subtype: GUID) -> bool {
        !self.decoders(category, major, subtype).is_empty()
    }
}

thread_local! {
    static COM: () = {
        // SAFETY: инициализация COM текущего потока; если поток уже STA (окно
        // оболочки) — вернётся `RPC_E_CHANGED_MODE`, COM при этом пригоден.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    };
}

/// Гарантировать, что COM в текущем потоке инициализирован.
pub(crate) fn ensure_com() {
    COM.with(|()| ());
}

/// Какие кодеки у ОС есть.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Availability {
    pub h264: bool,
    pub hevc: bool,
    pub aac: bool,
}

/// Есть ли на машине MFT для H.264, HEVC и AAC (один опрос ОС на процесс).
pub(crate) fn availability() -> Availability {
    static CACHE: OnceLock<Availability> = OnceLock::new();
    *CACHE.get_or_init(|| {
        let Some(api) = Api::get() else { return Availability { h264: false, hevc: false, aac: false } };
        ensure_com();
        let video = |sub: GUID| api.has_decoder(MFT_CATEGORY_VIDEO_DECODER, MFMediaType_Video, sub);
        Availability {
            h264: video(MFVideoFormat_H264),
            hevc: video(MFVideoFormat_HEVC),
            aac: api.has_decoder(MFT_CATEGORY_AUDIO_DECODER, MFMediaType_Audio, MFAudioFormat_AAC),
        }
    })
}


/// Видеобэкенд Media Foundation.
pub(crate) struct MfVideoBackend;

impl VideoBackend for MfVideoBackend {
    fn name(&self) -> &'static str {
        "media-foundation"
    }

    fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        match codec {
            Codec::H264 => availability().h264 && h264_profile_supported(codec_string),
            Codec::Hevc => availability().hevc && hevc_profile_supported(codec_string),
            _ => false,
        }
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn VideoFrameDecoder>, String> {
        Ok(Box::new(video::MfVideoDecoder::new(track)?))
    }
}

/// Аудиобэкенд Media Foundation.
pub(crate) struct MfAudioBackend;

impl AudioBackend for MfAudioBackend {
    fn name(&self) -> &'static str {
        "media-foundation"
    }

    fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        codec == Codec::Aac && availability().aac && aac_object_supported(codec_string)
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn AudioPcmDecoder>, String> {
        Ok(Box::new(audio::MfAacDecoder::new(track)?))
    }
}
