//! Hand-rolled FFI к FFmpeg (`libavformat`/`libavcodec`/`libavutil`/
//! `libswscale`) — не `bindgen`. ADR-030 измерил живой `bindgen`-блокер на
//! `x86_64-pc-windows-msvc` (AST→Rust-struct translation теряет размер для
//! набора структур FFmpeg и `libc::tm`); обход — объявлять руками только
//! те структуры и функции, которые реально нужны декодирующему циклу,
//! опираясь на реальные заголовки установленного FFmpeg 7.1 shared-dev
//! дистрибутива, а не догадки.
//!
//! Три категории структур:
//! - Полностью opaque (`AVFormatContext`, `AVCodec`, `AVCodecContext`,
//!   `AVCodecParameters`, `SwsContext`, `AVIOContext`) — код этого крейта
//!   передаёт только указатели на них между функциями FFmpeg, ни одного
//!   прямого чтения поля.
//! - "Head"-структуры (`AVFormatContextHead`, `AVStreamHead`,
//!   `AVFrameHead`) — ведущие поля реальной C-структуры до последнего поля,
//!   которое нужно читать, ABI FFmpeg добавляет поля только в хвост, так
//!   что более длинный реальный `sizeof` за пределами объявленного префикса
//!   не нарушает совместимость.
//! - `AVPacket` — исключение: FFmpeg документирует весь layout этой
//!   структуры как стабильный публичный API для прямого чтения/записи полей
//!   (`data`/`size`/`stream_index`/…), поэтому объявлена целиком.
#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
pub(crate) struct AVFormatContext {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct AVDictionary {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct AVCodec {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct AVCodecContext {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct AVCodecParameters {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct SwsContext {
    _private: [u8; 0],
}
#[repr(C)]
pub(crate) struct AVIOContext {
    _private: [u8; 0],
}

/// Ведущие поля `AVStream` (`libavformat/avformat.h`) до `duration`
/// включительно — единственные поля, которые вызывающая сторона читает:
/// `codecpar` (передаётся дальше в `avcodec_parameters_to_context`, сама
/// структура остаётся opaque) и `time_base`/`duration` для перевода pts в
/// секунды.
#[repr(C)]
pub(crate) struct AVStreamHead {
    pub av_class: *const c_void,
    pub index: c_int,
    pub id: c_int,
    pub codecpar: *mut AVCodecParameters,
    pub priv_data: *mut c_void,
    pub time_base_num: c_int,
    pub time_base_den: c_int,
    pub start_time: i64,
    pub duration: i64,
}

/// Ведущие поля `AVFormatContext` до `streams`/`nb_streams` включительно.
/// `pb` записывается перед `avformat_open_input` (custom-IO), `ctx_flags`
/// получает `AVFMT_FLAG_CUSTOM_IO`, `streams`/`nb_streams` читаются после
/// `av_find_best_stream`, чтобы дойти до `AVStream::codecpar` — accessor-
/// функции для этого пути в публичном API FFmpeg нет.
#[repr(C)]
pub(crate) struct AVFormatContextHead {
    pub av_class: *const c_void,
    pub iformat: *const c_void,
    pub oformat: *const c_void,
    pub priv_data: *mut c_void,
    pub pb: *mut AVIOContext,
    pub ctx_flags: c_int,
    pub nb_streams: u32,
    pub streams: *mut *mut AVStreamHead,
}

/// Полный публичный layout `AVPacket` (`libavcodec/packet.h`) — FFmpeg
/// документирует эти поля как стабильный API для прямого чтения/записи.
#[repr(C)]
pub(crate) struct AVPacket {
    pub buf: *mut c_void,
    pub pts: i64,
    pub dts: i64,
    pub data: *mut u8,
    pub size: c_int,
    pub stream_index: c_int,
    pub flags: c_int,
    pub side_data: *mut c_void,
    pub side_data_elems: c_int,
    pub duration: i64,
    pub pos: i64,
    pub opaque: *mut c_void,
    pub opaque_ref: *mut c_void,
    pub time_base: [c_int; 2],
}

/// Ведущие поля `AVFrame` (`libavutil/frame.h`) до `pts` включительно.
/// Порядок подтверждён живым прогоном (срез 4): `sample_aspect_ratio`
/// (два `c_int`) стоит между `pict_type` и `pts` — его пропуск в более
/// ранней версии этого слоя ломал чтение `pts` (наблюдалось как заведомо
/// невозможное значение вроде `pts ≈ 2^32`).
#[repr(C)]
pub(crate) struct AVFrameHead {
    pub data: [*mut u8; 8],
    pub linesize: [c_int; 8],
    pub extended_data: *mut *mut u8,
    pub width: c_int,
    pub height: c_int,
    pub nb_samples: c_int,
    pub format: c_int,
    pub key_frame: c_int,
    pub pict_type: c_int,
    pub sample_aspect_ratio: [c_int; 2],
    pub pts: i64,
}

/// Минимальный layout `AVChannelLayout` (`libavutil/channel_layout.h`) —
/// только ведущие поля `order`/`nb_channels`, которые нужны срезу 12
/// (детект числа каналов аудиодорожки). `u_mask_or_map`/`opaque` — хвост
/// структуры (union `mask`/`map` + heap-указатель для custom-порядка),
/// объявлен целиком (не "head"), потому что `av_opt_get_chlayout` пишет во
/// все 24 байта структуры — если передать укороченный тип, FFmpeg запишет
/// за границы аллокации. Подтверждено живым прогоном (срез 12,
/// `D:\Temp\ffmpeg-ffi-poc`): `nb_channels == 2` на `test.mp4` (aac,
/// stereo), совпадает с `ffprobe`.
#[repr(C)]
#[derive(Default)]
pub(crate) struct AVChannelLayout {
    pub order: c_int,
    pub nb_channels: c_int,
    u_mask_or_map: u64,
    opaque: *mut c_void,
}

pub(crate) const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub(crate) const AVMEDIA_TYPE_AUDIO: c_int = 1;
/// `AV_PIX_FMT_RGBA` (`libavutil/pixfmt.h`).
pub(crate) const AV_PIX_FMT_RGBA: c_int = 26;
pub(crate) const SWS_BILINEAR: c_int = 4;
/// "The caller has supplied a custom AVIOContext, don't avio_close() it."
pub(crate) const AVFMT_FLAG_CUSTOM_IO: c_int = 0x0080;
/// `FFERRTAG('E','O','F',' ')` — FFmpeg не экспортирует эту константу как
/// символ, только как макрос в заголовке, поэтому она посчитана руками
/// (см. `libavutil/error.h`) и подтверждена живым прогоном (custom-IO
/// `read_packet` callback действительно останавливает демуксер при возврате
/// этого значения).
pub(crate) const AVERROR_EOF: c_int = -541_478_725;
pub(crate) const AVSEEK_FLAG_BACKWARD: c_int = 1;
pub(crate) const AV_NOPTS_VALUE: i64 = i64::MIN;

pub(crate) type ReadPacketFn = extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int;
pub(crate) type WritePacketFn = extern "C" fn(*mut c_void, *const u8, c_int) -> c_int;
pub(crate) type SeekFn = extern "C" fn(*mut c_void, i64, c_int) -> i64;

// SAFETY: каждое объявление ниже транскрибировано вручную с реальных
// заголовков установленного FFmpeg 7.1 shared-dev дистрибутива (см.
// module-doc выше) — сигнатуры (типы аргументов/возврата) соответствуют
// `libavformat`/`libavcodec`/`libavutil`/`libswscale` C API; корректность
// вызова (валидность указателей, порядок вызовов) — забота каждого сайта
// вызова в `decoder.rs`, у каждого свой `// SAFETY:`.
unsafe extern "C" {
    pub(crate) fn avformat_alloc_context() -> *mut AVFormatContext;
    pub(crate) fn avformat_open_input(
        ctx: *mut *mut AVFormatContext,
        url: *const c_char,
        fmt: *const c_void,
        options: *mut *mut AVDictionary,
    ) -> c_int;
    pub(crate) fn avformat_find_stream_info(
        ctx: *mut AVFormatContext,
        options: *mut *mut c_void,
    ) -> c_int;
    pub(crate) fn avformat_close_input(ctx: *mut *mut AVFormatContext);
    pub(crate) fn av_strerror(errnum: c_int, buf: *mut c_char, buf_size: usize) -> c_int;

    pub(crate) fn av_find_best_stream(
        ic: *mut AVFormatContext,
        media_type: c_int,
        wanted_stream_nb: c_int,
        related_stream: c_int,
        decoder_ret: *mut *const AVCodec,
        flags: c_int,
    ) -> c_int;

    pub(crate) fn avcodec_alloc_context3(codec: *const AVCodec) -> *mut AVCodecContext;
    pub(crate) fn avcodec_parameters_to_context(
        codec_ctx: *mut AVCodecContext,
        par: *const AVCodecParameters,
    ) -> c_int;
    pub(crate) fn avcodec_open2(
        ctx: *mut AVCodecContext,
        codec: *const AVCodec,
        options: *mut *mut AVDictionary,
    ) -> c_int;
    pub(crate) fn avcodec_free_context(ctx: *mut *mut AVCodecContext);
    pub(crate) fn avcodec_send_packet(ctx: *mut AVCodecContext, pkt: *const AVPacket) -> c_int;
    pub(crate) fn avcodec_receive_frame(
        ctx: *mut AVCodecContext,
        frame: *mut AVFrameHead,
    ) -> c_int;
    pub(crate) fn avcodec_flush_buffers(ctx: *mut AVCodecContext);

    /// Generic `AVOption` getter (`libavutil/opt.h`) — читает поле по имени
    /// опции через `AVClass`-рефлексию, без знания реального смещения поля
    /// внутри `AVCodecContext`. Имя опции для частоты дискретизации — `"ar"`
    /// (алиас `sample_rate` в реестре опций не зарегистрирован, подтверждено
    /// живым прогоном срез 12 — `av_opt_get_int(.., "sample_rate", ..)`
    /// возвращает `AVERROR_OPTION_NOT_FOUND`, `"ar"` — `0`).
    pub(crate) fn av_opt_get_int(
        obj: *mut c_void,
        name: *const c_char,
        search_flags: c_int,
        out_val: *mut i64,
    ) -> c_int;
    /// Тот же механизм для `AVChannelLayout` — имя опции `"ch_layout"`.
    pub(crate) fn av_opt_get_chlayout(
        obj: *mut c_void,
        name: *const c_char,
        search_flags: c_int,
        layout: *mut AVChannelLayout,
    ) -> c_int;
    /// Освобождает heap-аллокацию `AVChannelLayout` для custom-порядка
    /// каналов (`order == AV_CHANNEL_ORDER_CUSTOM`) — no-op для
    /// mask-based layout (единственный путь, который срез 12 читает), но
    /// вызывается безусловно, чтобы не завязываться на это предположение.
    pub(crate) fn av_channel_layout_uninit(layout: *mut AVChannelLayout);

    pub(crate) fn av_packet_alloc() -> *mut AVPacket;
    pub(crate) fn av_packet_free(pkt: *mut *mut AVPacket);
    pub(crate) fn av_packet_unref(pkt: *mut AVPacket);
    pub(crate) fn av_read_frame(ctx: *mut AVFormatContext, pkt: *mut AVPacket) -> c_int;
    pub(crate) fn av_seek_frame(
        ctx: *mut AVFormatContext,
        stream_index: c_int,
        timestamp: i64,
        flags: c_int,
    ) -> c_int;

    pub(crate) fn av_frame_alloc() -> *mut AVFrameHead;
    pub(crate) fn av_frame_free(frame: *mut *mut AVFrameHead);

    pub(crate) fn av_malloc(size: usize) -> *mut c_void;
    pub(crate) fn av_free(ptr: *mut c_void);

    pub(crate) fn avio_alloc_context(
        buffer: *mut u8,
        buffer_size: c_int,
        write_flag: c_int,
        opaque: *mut c_void,
        read_packet: Option<ReadPacketFn>,
        write_packet: Option<WritePacketFn>,
        seek: Option<SeekFn>,
    ) -> *mut AVIOContext;
    pub(crate) fn avio_context_free(s: *mut *mut AVIOContext);

    pub(crate) fn sws_getContext(
        src_w: c_int,
        src_h: c_int,
        src_fmt: c_int,
        dst_w: c_int,
        dst_h: c_int,
        dst_fmt: c_int,
        flags: c_int,
        src_filter: *mut c_void,
        dst_filter: *mut c_void,
        param: *const f64,
    ) -> *mut SwsContext;
    pub(crate) fn sws_scale(
        ctx: *mut SwsContext,
        src_slice: *const *const u8,
        src_stride: *const c_int,
        src_slice_y: c_int,
        src_slice_h: c_int,
        dst_slice: *const *mut u8,
        dst_stride: *const c_int,
    ) -> c_int;
    pub(crate) fn sws_freeContext(ctx: *mut SwsContext);
}

/// Переводит код ошибки FFmpeg (обычно отрицательный `errno`-подобный код)
/// в человекочитаемую строку через `av_strerror`.
pub(crate) fn describe_error(ret: c_int) -> String {
    let mut buf = [0i8; 256];
    // SAFETY: `buf` — стековый массив ровно `buf.len()` байт; контракт
    // `av_strerror` — не писать за пределы переданного `buf_size`.
    unsafe {
        av_strerror(ret, buf.as_mut_ptr(), buf.len());
    }
    // SAFETY: `av_strerror` всегда NUL-терминирует `buf` — как для
    // известного кода, так и для fallback-строки на неизвестном коде
    // (контракт FFmpeg, `libavutil/error.c`).
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
