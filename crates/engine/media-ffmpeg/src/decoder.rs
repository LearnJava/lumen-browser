//! `VideoDecoder`/`VideoDecodeSession` (`lumen_core::ext`) поверх
//! hand-rolled FFI из [`crate::ffi`]. Единственная реализация этих
//! trait-anchor'ов в дереве — GAP-MEDIADECODE, ADR-030.
//!
//! Контейнер приходит вызывающей стороне как `&[u8]` (тело сетевого
//! ответа/Blob), не как путь на диске — `open()` заводит FFmpeg-у
//! custom `AVIOContext` поверх собственного `BufferReader`, а не
//! временный файл.

use std::ffi::{c_int, c_void, CString};
use std::ptr;

use lumen_core::ext::{AudioTrackInfo, VideoDecodeSession, VideoDecoder};

use crate::ffi::{
    av_channel_layout_uninit, av_find_best_stream, av_frame_alloc, av_frame_free, av_free,
    av_malloc, av_opt_get_chlayout, av_opt_get_int, av_packet_alloc, av_packet_free,
    av_packet_unref, av_read_frame, av_seek_frame, avcodec_alloc_context3, avcodec_flush_buffers,
    avcodec_free_context, avcodec_open2, avcodec_parameters_to_context, avcodec_receive_frame,
    avcodec_send_packet, avformat_alloc_context, avformat_close_input,
    avformat_find_stream_info, avformat_open_input, avio_alloc_context, avio_context_free,
    describe_error, sws_freeContext, sws_getContext, sws_scale, AVChannelLayout, AVCodec,
    AVCodecContext, AVFormatContext, AVFormatContextHead, AVFrameHead, AVIOContext,
    AVMEDIA_TYPE_AUDIO, AVMEDIA_TYPE_VIDEO, AVSEEK_FLAG_BACKWARD, AVFMT_FLAG_CUSTOM_IO,
    AVERROR_EOF, AV_NOPTS_VALUE, AV_PIX_FMT_RGBA, SWS_BILINEAR, AV_SAMPLE_FMT_FLT,
    AV_SAMPLE_FMT_FLTP, AV_SAMPLE_FMT_S16, AV_SAMPLE_FMT_S16P, AV_SAMPLE_FMT_U8,
    AV_SAMPLE_FMT_U8P,
};

/// `FFmpeg`-бэкенд `VideoDecoder` (ADR-030). Существует только под feature
/// `ffmpeg` — без неё крейт компилируется, но не экспортирует этот тип
/// (`build.rs` не линкует FFmpeg вовсе, символы не резолвятся).
#[derive(Debug, Default)]
pub struct FfmpegVideoDecoder;

impl VideoDecoder for FfmpegVideoDecoder {
    fn decoder_name(&self) -> &'static str {
        "ffmpeg"
    }

    fn mime_types(&self) -> &'static [&'static str] {
        &["video/mp4", "video/webm", "video/ogg"]
    }

    fn open(&self, bytes: &[u8]) -> Result<Box<dyn VideoDecodeSession>, String> {
        let session = FfmpegSession::open(bytes.to_vec())?;
        Ok(Box::new(session))
    }
}

/// Байты контейнера, читаемые FFmpeg через custom `AVIOContext` вместо
/// пути на диске. Указатель на этот `Box` живёт как `opaque` в
/// `AVIOContext` ровно до вызова [`avio_context_free`] в `Drop` сессии.
struct BufferReader {
    data: Vec<u8>,
    pos: usize,
}

extern "C" fn read_packet_cb(opaque: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int {
    // SAFETY: `opaque` — указатель на `BufferReader`, выделенный через
    // `Box::into_raw` в `FfmpegSession::open` и не освобождаемый, пока жив
    // связанный `AVIOContext`; FFmpeg вызывает этот callback только между
    // `avio_alloc_context` и `avio_context_free` (см. `Drop` для
    // `FfmpegSession`), так что указатель всегда валиден на момент вызова.
    let reader = unsafe { &mut *opaque.cast::<BufferReader>() };
    let remaining = reader.data.len().saturating_sub(reader.pos);
    if remaining == 0 {
        return AVERROR_EOF;
    }
    let n = remaining.min(usize::try_from(buf_size.max(0)).unwrap_or(0));
    // SAFETY: `buf` — буфер размера как минимум `buf_size`, выделенный
    // самим FFmpeg перед вызовом (контракт `read_packet` в `avio.h`); `n`
    // не превышает ни `buf_size`, ни числа оставшихся байт в `reader.data`,
    // так что и чтение, и запись остаются в границах своих аллокаций.
    unsafe {
        ptr::copy_nonoverlapping(reader.data.as_ptr().add(reader.pos), buf, n);
    }
    reader.pos += n;
    n as c_int
}

extern "C" fn seek_cb(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    const SEEK_SET: c_int = 0;
    const SEEK_CUR: c_int = 1;
    const SEEK_END: c_int = 2;
    const AVSEEK_SIZE: c_int = 0x10000;
    // SAFETY: тот же инвариант, что и в `read_packet_cb` выше.
    let reader = unsafe { &mut *opaque.cast::<BufferReader>() };
    if whence == AVSEEK_SIZE {
        return reader.data.len() as i64;
    }
    let base: i64 = match whence {
        SEEK_SET => 0,
        SEEK_CUR => reader.pos as i64,
        SEEK_END => reader.data.len() as i64,
        _ => return -1,
    };
    let new_pos = base.saturating_add(offset);
    if new_pos < 0 || new_pos as usize > reader.data.len() {
        return -1;
    }
    reader.pos = new_pos as usize;
    new_pos
}

/// Конвертирует один PCM-сэмпл с плавающей точкой (`AV_SAMPLE_FMT_FLT`/
/// `_FLTP`, ожидаемый диапазон `[-1.0, 1.0]`) в `i16` с насыщением —
/// значения за пределами диапазона (перегруз кодека/контейнера) обрезаются
/// вместо переполнения при масштабировании на `i16::MAX`.
fn f32_sample_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

/// Конвертирует один PCM-сэмпл `AV_SAMPLE_FMT_U8`/`_U8P` (беззнаковый,
/// центр молчания на 128) в `i16` (центр молчания на 0).
fn u8_sample_to_i16(sample: u8) -> i16 {
    (i16::from(sample) - 128) * 256
}

/// Одна открытая сессия декодирования — владеет FFmpeg-контекстами и
/// байтами контейнера на всё время своей жизни; `Drop` освобождает их в
/// порядке, обратном созданию (кодек → демуксер → custom IO → буфер байт).
pub struct FfmpegSession {
    fmt_ctx: *mut AVFormatContext,
    codec_ctx: *mut AVCodecContext,
    avio_ctx: *mut AVIOContext,
    reader: *mut BufferReader,
    stream_index: c_int,
    time_base_num: c_int,
    time_base_den: c_int,
    width: u32,
    height: u32,
    duration_secs: Option<f64>,
    /// Первый декодированный кадр, снятый уже в `open()` — чтобы
    /// `dimensions()` не лгала до первого `frame_at()`, и чтобы
    /// `frame_at(0.0)` сразу после `open()` не платил за повторный seek.
    first_frame: (f64, Vec<u8>),
    /// Метаданные аудиодорожки, снятые один раз в `open()` (срез 12).
    audio_track: Option<AudioTrackInfo>,
    /// Индекс аудиопотока в `fmt_ctx`, `-1` — аудиодорожки нет/не декодируется
    /// (срез 13). Отдельный от `stream_index` (видео) — оба читаются из
    /// одного и того же демуксера через общий `av_read_frame`.
    audio_stream_index: c_int,
    /// Открытый (`avcodec_open2`) кодек-контекст аудиодорожки, `null` —
    /// аудиодорожки нет/не декодируется. В отличие от среза 12 (закрывался
    /// сразу после снятия метаданных), срез 13 держит его открытым на всё
    /// время жизни сессии, чтобы `decode_audio_pcm` могло декодировать PCM
    /// без повторного `avcodec_open2`.
    audio_codec_ctx: *mut AVCodecContext,
}

// SAFETY: `FfmpegSession` — единственный владелец всех перечисленных
// указателей (FFmpeg-контексты + `BufferReader`); ничего не разделяет их
// одновременно с двух потоков, а `VideoDecodeSession`-методы берут
// `&mut self`, так что одновременного доступа с разных потоков контракт
// вызывающей стороны (owned `Box<dyn VideoDecodeSession>`, одна очередь
// декодирования на медиа-элемент) не допускает — тот же аргумент, что и у
// `Icu4xUnicodeProvider`/`OwnedNativeFn` в этом workspace.
unsafe impl Send for FfmpegSession {}

impl FfmpegSession {
    fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let reader = Box::into_raw(Box::new(BufferReader { data: bytes, pos: 0 }));

        const AVIO_BUF_SIZE: usize = 4096;
        // SAFETY: `av_malloc` — обычная C-аллокация без предусловий на
        // входные данные кроме размера; возвращаемый указатель проверяется
        // на null сразу ниже.
        let avio_buf = unsafe { av_malloc(AVIO_BUF_SIZE) }.cast::<u8>();
        if avio_buf.is_null() {
            // SAFETY: `reader` только что создан этой же функцией через
            // `Box::into_raw` и ещё не передан ни одному FFmpeg-callback'у —
            // возврат владения обратно `Box` для немедленного drop корректен.
            unsafe {
                drop(Box::from_raw(reader));
            }
            return Err("FFmpeg: av_malloc не смог выделить буфер AVIOContext".to_string());
        }

        // SAFETY: `avio_buf` — только что выделенные `av_malloc` `AVIO_BUF_SIZE`
        // байт, `reader` — валидный указатель на `BufferReader`, живущий как
        // минимум до `avio_context_free` ниже; `read_packet_cb`/`seek_cb` —
        // `extern "C" fn` с сигнатурой, которую ожидает `avio_alloc_context`.
        let avio_ctx = unsafe {
            avio_alloc_context(
                avio_buf,
                AVIO_BUF_SIZE as c_int,
                0,
                reader.cast::<c_void>(),
                Some(read_packet_cb),
                None,
                Some(seek_cb),
            )
        };
        if avio_ctx.is_null() {
            // SAFETY: `avio_alloc_context` не взял владение `avio_buf` при
            // отказе (вернул null до сохранения указателя) — освобождаем
            // его тем же аллокатором, которым выделяли.
            unsafe {
                av_free(avio_buf.cast::<c_void>());
                drop(Box::from_raw(reader));
            }
            return Err("FFmpeg: avio_alloc_context вернул null".to_string());
        }

        // SAFETY: `avformat_alloc_context` — обычная C-аллокация без
        // предусловий; null проверяется сразу ниже, до записи полей.
        let mut fmt_ctx = unsafe { avformat_alloc_context() };
        if fmt_ctx.is_null() {
            // SAFETY: ни один из ресурсов выше ещё не передан
            // `avformat_open_input` — освобождаем их напрямую в обратном
            // порядке создания.
            unsafe {
                let mut avio_ctx = avio_ctx;
                avio_context_free(&mut avio_ctx);
                drop(Box::from_raw(reader));
            }
            return Err("FFmpeg: avformat_alloc_context вернул null".to_string());
        }

        // SAFETY: `fmt_ctx` только что выделен `avformat_alloc_context` и
        // ещё не передан `avformat_open_input` — прямая запись `pb`/
        // `ctx_flags` через head-структуру, чьи поля объявлены в том же
        // порядке, что и реальный `AVFormatContext` (см. `ffi.rs`), корректна.
        unsafe {
            let head = fmt_ctx.cast::<AVFormatContextHead>();
            (*head).pb = avio_ctx;
            (*head).ctx_flags |= AVFMT_FLAG_CUSTOM_IO;
        }

        let empty_url = CString::new("").unwrap_or_default();
        // SAFETY: `fmt_ctx` — валидный, предварительно сконфигурированный
        // (custom IO) указатель на `*mut AVFormatContext`; `empty_url` живёт
        // до конца этого вызова. При ошибке `avformat_open_input`
        // освобождает `*fmt_ctx` сам и пишет туда null (контракт FFmpeg), но
        // НЕ трогает `pb`/`avio_ctx` из-за `AVFMT_FLAG_CUSTOM_IO` — их
        // освобождает вызывающая сторона в обеих ветках ниже.
        let open_ret =
            unsafe { avformat_open_input(&mut fmt_ctx, empty_url.as_ptr(), ptr::null(), ptr::null_mut()) };
        if open_ret < 0 {
            let msg = describe_error(open_ret);
            // SAFETY: `avformat_open_input` уже освободил `*fmt_ctx` при
            // отказе (contract) — здесь освобождаются только ресурсы,
            // которые FFmpeg не тронул из-за custom-IO флага.
            unsafe {
                let mut avio_ctx = avio_ctx;
                avio_context_free(&mut avio_ctx);
                drop(Box::from_raw(reader));
            }
            return Err(format!("FFmpeg: avformat_open_input: {msg}"));
        }

        let cleanup_after_open = |fmt_ctx: *mut AVFormatContext| {
            let mut fmt_ctx = fmt_ctx;
            // SAFETY: вызывается только из веток отказа ниже, до передачи
            // владения наружу в `Ok(FfmpegSession { .. })` — `fmt_ctx`/
            // `avio_ctx`/`reader` в этот момент ещё не имеют других
            // владельцев.
            unsafe {
                avformat_close_input(&mut fmt_ctx);
                let mut avio_ctx = avio_ctx;
                avio_context_free(&mut avio_ctx);
                drop(Box::from_raw(reader));
            }
        };

        // SAFETY: `fmt_ctx` успешно открыт вызовом выше и ещё жив.
        let find_ret = unsafe { avformat_find_stream_info(fmt_ctx, ptr::null_mut()) };
        if find_ret < 0 {
            let msg = describe_error(find_ret);
            cleanup_after_open(fmt_ctx);
            return Err(format!("FFmpeg: avformat_find_stream_info: {msg}"));
        }

        let mut decoder: *const AVCodec = ptr::null();
        // SAFETY: `fmt_ctx` содержит информацию о потоках после успешного
        // `avformat_find_stream_info` выше; `&mut decoder` — валидный
        // указатель на локальную переменную этого стека.
        let stream_index = unsafe {
            av_find_best_stream(fmt_ctx, AVMEDIA_TYPE_VIDEO, -1, -1, &mut decoder, 0)
        };
        if stream_index < 0 {
            let msg = describe_error(stream_index);
            cleanup_after_open(fmt_ctx);
            return Err(format!(
                "FFmpeg: не найдена декодируемая видеодорожка (dedicated media source failure steps): {msg}"
            ));
        }

        // SAFETY: `stream_index` в границах `nb_streams` — гарантия
        // `av_find_best_stream` при неотрицательном возврате; `fmt_ctx`
        // жив, `streams`/`codecpar` читаются по head-layout из `ffi.rs`.
        let (codecpar, time_base_num, time_base_den, duration_secs) = unsafe {
            let head = fmt_ctx.cast::<AVFormatContextHead>();
            let stream = *(*head).streams.add(stream_index as usize);
            let tb_num = (*stream).time_base_num;
            let tb_den = (*stream).time_base_den;
            let duration_secs = if (*stream).duration == AV_NOPTS_VALUE || tb_den == 0 {
                None
            } else {
                Some((*stream).duration as f64 * f64::from(tb_num) / f64::from(tb_den))
            };
            ((*stream).codecpar, tb_num, tb_den, duration_secs)
        };

        // SAFETY: `decoder` — не-null указатель на `AVCodec`, вернувшийся
        // из успешного `av_find_best_stream` выше.
        let codec_ctx = unsafe { avcodec_alloc_context3(decoder) };
        if codec_ctx.is_null() {
            cleanup_after_open(fmt_ctx);
            return Err("FFmpeg: avcodec_alloc_context3 вернул null".to_string());
        }

        // SAFETY: `codec_ctx` только что выделен, `codecpar` — валидный
        // указатель из того же потока, что и `decoder` (оба получены из
        // одного `AVStream` через `av_find_best_stream`).
        let params_ret = unsafe { avcodec_parameters_to_context(codec_ctx, codecpar) };
        if params_ret < 0 {
            let msg = describe_error(params_ret);
            // SAFETY: `codec_ctx` не открыт (`avcodec_open2` ещё не
            // вызывался) — `avcodec_free_context` безопасно освобождает
            // только что выделенный контекст.
            unsafe {
                let mut codec_ctx = codec_ctx;
                avcodec_free_context(&mut codec_ctx);
            }
            cleanup_after_open(fmt_ctx);
            return Err(format!("FFmpeg: avcodec_parameters_to_context: {msg}"));
        }

        // SAFETY: `codec_ctx` сконфигурирован предыдущим вызовом, `decoder`
        // — тот же кодек, что и при `avcodec_alloc_context3`.
        let open_codec_ret = unsafe { avcodec_open2(codec_ctx, decoder, ptr::null_mut()) };
        if open_codec_ret < 0 {
            let msg = describe_error(open_codec_ret);
            // SAFETY: `avcodec_open2` не переходит в открытое состояние
            // при ошибке — `avcodec_free_context` освобождает контекст в
            // допустимом для него состоянии.
            unsafe {
                let mut codec_ctx = codec_ctx;
                avcodec_free_context(&mut codec_ctx);
            }
            cleanup_after_open(fmt_ctx);
            return Err(format!("FFmpeg: avcodec_open2: {msg}"));
        }

        // SAFETY: `fmt_ctx` — тот же живой демуксер, что и выше;
        // `open_audio_track` не трогает позицию чтения демуксера (не
        // вызывает `av_read_frame`), так что не мешает `av_find_best_stream`
        // видео-дорожки/декодированию первого видеокадра ниже.
        let (audio_stream_index, audio_codec_ctx, audio_track) =
            match unsafe { Self::open_audio_track(fmt_ctx) } {
                Some((idx, ctx, info)) => (idx, ctx, Some(info)),
                None => (-1, ptr::null_mut(), None),
            };

        let mut session = Self {
            fmt_ctx,
            codec_ctx,
            avio_ctx,
            reader,
            stream_index,
            time_base_num,
            time_base_den,
            width: 0,
            height: 0,
            duration_secs,
            first_frame: (0.0, Vec::new()),
            audio_track,
            audio_stream_index,
            audio_codec_ctx,
        };

        let (w, h, rgba) = session.decode_from_current_position(0.0).map_err(|e| {
            format!("FFmpeg: контейнер открыт, но ни один видеокадр не декодируется: {e}")
        })?;
        session.width = w;
        session.height = h;
        session.first_frame = (0.0, rgba);

        Ok(session)
    }

    /// Ищет первую декодируемую аудиодорожку, снимает её метаданные
    /// (`sample_rate`/`channels`) через generic `AVOption`-геттеры
    /// (`av_opt_get_int("ar")`/`av_opt_get_chlayout("ch_layout")`) и, в
    /// отличие от среза 12, оставляет `AVCodecContext` открытым (не
    /// закрывает его) — срез 13's `decode_audio_pcm` декодирует PCM через
    /// него же, без повторного `avcodec_alloc_context3`/`avcodec_open2`.
    /// Отсутствие аудиодорожки/недекодируемый аудиокодек/нечитаемые
    /// метаданные — не ошибка, `None` (и тогда контекст, если он успел
    /// открыться, освобождается здесь же).
    ///
    /// # Safety
    /// `fmt_ctx` — валидный, открытый `avformat_open_input`+
    /// `avformat_find_stream_info` демуксер.
    unsafe fn open_audio_track(
        fmt_ctx: *mut AVFormatContext,
    ) -> Option<(c_int, *mut AVCodecContext, AudioTrackInfo)> {
        let mut audio_decoder: *const AVCodec = ptr::null();
        // SAFETY: `fmt_ctx` валиден по контракту функции; `&mut audio_decoder`
        // — валидный указатель на локальную переменную этого стека.
        let audio_stream_index = unsafe {
            av_find_best_stream(fmt_ctx, AVMEDIA_TYPE_AUDIO, -1, -1, &mut audio_decoder, 0)
        };
        if audio_stream_index < 0 {
            return None;
        }

        // SAFETY: `audio_stream_index` в границах `nb_streams` — гарантия
        // `av_find_best_stream` при неотрицательном возврате; `fmt_ctx`
        // жив, `streams`/`codecpar` читаются по head-layout из `ffi.rs`.
        let codecpar = unsafe {
            let head = fmt_ctx.cast::<AVFormatContextHead>();
            let stream = *(*head).streams.add(audio_stream_index as usize);
            (*stream).codecpar
        };

        // SAFETY: `audio_decoder` — не-null указатель на `AVCodec` из
        // успешного `av_find_best_stream` выше.
        let audio_codec_ctx = unsafe { avcodec_alloc_context3(audio_decoder) };
        if audio_codec_ctx.is_null() {
            return None;
        }
        // SAFETY: `audio_codec_ctx` только что выделен, `codecpar` —
        // валидный указатель из того же потока, что и `audio_decoder`.
        let params_ret = unsafe { avcodec_parameters_to_context(audio_codec_ctx, codecpar) };
        if params_ret < 0 {
            // SAFETY: `avcodec_open2` ещё не вызывался — контекст в
            // допустимом для `avcodec_free_context` состоянии.
            unsafe {
                let mut ctx = audio_codec_ctx;
                avcodec_free_context(&mut ctx);
            }
            return None;
        }
        // SAFETY: `audio_codec_ctx` сконфигурирован предыдущим вызовом,
        // `audio_decoder` — тот же кодек, что и при `avcodec_alloc_context3`.
        let open_ret = unsafe { avcodec_open2(audio_codec_ctx, audio_decoder, ptr::null_mut()) };
        if open_ret < 0 {
            // SAFETY: `avcodec_open2` не переходит в открытое состояние при
            // ошибке — `avcodec_free_context` освобождает контекст в
            // допустимом для него состоянии.
            unsafe {
                let mut ctx = audio_codec_ctx;
                avcodec_free_context(&mut ctx);
            }
            return None;
        }

        let ar_name = CString::new("ar").unwrap_or_default();
        let mut sample_rate: i64 = -1;
        // SAFETY: `audio_codec_ctx` открыт `avcodec_open2` выше — его
        // `AVClass`-таблица опций (унаследованная от базового
        // `AVCodecContext`) содержит опцию `"ar"`, подтверждено живым
        // прогоном (срез 12, `D:\Temp\ffmpeg-ffi-poc`); `ar_name` живёт до
        // конца этого вызова, `&mut sample_rate` — валидный указатель на
        // локальную переменную.
        let sample_rate_ret = unsafe {
            av_opt_get_int(audio_codec_ctx.cast::<c_void>(), ar_name.as_ptr(), 0, &mut sample_rate)
        };

        let ch_layout_name = CString::new("ch_layout").unwrap_or_default();
        let mut layout = AVChannelLayout::default();
        // SAFETY: то же самое, что и выше, для опции `"ch_layout"`; `layout`
        // — стековая переменная ровно размера `AVChannelLayout` (24 байта,
        // см. `ffi.rs`), `av_opt_get_chlayout` пишет не больше этого объёма.
        let ch_layout_ret = unsafe {
            av_opt_get_chlayout(audio_codec_ctx.cast::<c_void>(), ch_layout_name.as_ptr(), 0, &mut layout)
        };
        let channels = if ch_layout_ret >= 0 && layout.nb_channels > 0 {
            Some(layout.nb_channels as u16)
        } else {
            None
        };
        // SAFETY: `layout` было заполнено (или оставлено нулевым при
        // отказе) вызовом выше — `av_channel_layout_uninit` документированно
        // безопасен и на нулевой/mask-based layout (единственный путь без
        // heap-аллокации, no-op в этом случае).
        unsafe {
            av_channel_layout_uninit(&mut layout);
        }

        match (sample_rate_ret, channels) {
            (ret, Some(channels)) if ret >= 0 && sample_rate > 0 => Some((
                audio_stream_index,
                audio_codec_ctx,
                AudioTrackInfo { sample_rate: sample_rate as u32, channels },
            )),
            _ => {
                // SAFETY: метаданные нечитаемы — этот контекст не будет
                // сохранён вызывающей стороной (`open()` хранит его только
                // при `Some`), освобождаем сами, чтобы не утечь.
                unsafe {
                    let mut ctx = audio_codec_ctx;
                    avcodec_free_context(&mut ctx);
                }
                None
            }
        }
    }

    /// Декодирует следующую порцию PCM аудиодорожки, продолжая от текущей
    /// позиции чтения `fmt_ctx` (общей с видеодорожкой — `frame_at`,
    /// вызванный до этого метода, продвигает и её), до накопления
    /// `max_samples` сэмплов на канал или EOF. Без `swresample`: формат
    /// сэмплов декодера (`frame.format`) конвертируется в интерливленный
    /// S16 вручную — [`Self::frame_to_interleaved_i16`] знает конечный
    /// список форматов, остальные — явная ошибка, а не тихое искажение
    /// звука.
    fn decode_audio_pcm(&mut self, max_samples: usize) -> Result<Vec<i16>, String> {
        if self.audio_stream_index < 0 || self.audio_codec_ctx.is_null() {
            return Err("контейнер не несёт декодируемой аудиодорожки".to_string());
        }
        let Some(channels) = self.audio_track.map(|t| t.channels as usize) else {
            return Err("метаданные аудиодорожки недоступны".to_string());
        };
        if channels == 0 || channels > 8 {
            return Err(format!(
                "FFmpeg: неподдерживаемое число каналов аудио ({channels}) — AVFrameHead.data вмещает не больше 8 планов"
            ));
        }

        // SAFETY: `av_packet_alloc` — обычная C-аллокация; null проверяется
        // сразу ниже перед любым использованием.
        let pkt = unsafe { av_packet_alloc() };
        // SAFETY: `av_frame_alloc` — обычная C-аллокация; null проверяется
        // сразу ниже перед любым использованием.
        let frame = unsafe { av_frame_alloc() };
        if pkt.is_null() || frame.is_null() {
            // SAFETY: `av_packet_free`/`av_frame_free` документированно
            // принимают указатель на null-переменную как no-op.
            unsafe {
                let mut pkt = pkt;
                av_packet_free(&mut pkt);
                let mut frame = frame;
                av_frame_free(&mut frame);
            }
            return Err("av_packet_alloc/av_frame_alloc вернул null".to_string());
        }

        let target_len = max_samples.saturating_mul(channels);
        let mut samples: Vec<i16> = Vec::new();
        let mut convert_err: Option<String> = None;
        'outer: while samples.len() < target_len {
            // SAFETY: `self.fmt_ctx` открыт и жив на весь срок жизни
            // `self`; `pkt` — валидный, только что выделенный `AVPacket`.
            let read_ret = unsafe { av_read_frame(self.fmt_ctx, pkt) };
            if read_ret < 0 {
                break; // EOF демуксера.
            }
            // SAFETY: `pkt` заполнен успешным `av_read_frame` выше.
            let pkt_stream = unsafe { (*pkt).stream_index };
            if pkt_stream != self.audio_stream_index {
                // SAFETY: `pkt` — тот же валидный пакет, `av_packet_unref`
                // — штатный способ освободить его данные без деалокации
                // самой структуры (она переиспользуется в цикле).
                unsafe {
                    av_packet_unref(pkt);
                }
                continue;
            }
            // SAFETY: `self.audio_codec_ctx` открыт `avcodec_open2` в
            // `open_audio_track`; `pkt` содержит данные аудиодорожки.
            let send_ret = unsafe { avcodec_send_packet(self.audio_codec_ctx, pkt) };
            // SAFETY: `pkt` больше не нужен после `avcodec_send_packet`
            // (FFmpeg копирует/референсит данные внутри), безопасно
            // освободить перед следующей итерацией.
            unsafe {
                av_packet_unref(pkt);
            }
            if send_ret < 0 {
                continue;
            }
            loop {
                // SAFETY: `self.audio_codec_ctx` открыт; `frame` —
                // валидный, выделенный выше `AVFrame`, переиспользуемый
                // между попытками приёма.
                let recv_ret = unsafe { avcodec_receive_frame(self.audio_codec_ctx, frame) };
                if recv_ret != 0 {
                    break;
                }
                // SAFETY: `avcodec_receive_frame` вернул успех — поля
                // `frame` заполнены декодером по заявленному в `ffi.rs`
                // layout'у.
                let (fmt, nb_samples) = unsafe { ((*frame).format, (*frame).nb_samples) };
                if nb_samples <= 0 {
                    continue;
                }
                // SAFETY: `frame` только что успешно декодирован
                // `avcodec_receive_frame` выше, `fmt`/`nb_samples` —
                // значения из того же `frame`.
                match unsafe {
                    Self::frame_to_interleaved_i16(frame, fmt, nb_samples as usize, channels)
                } {
                    Ok(chunk) => samples.extend(chunk),
                    Err(e) => {
                        convert_err = Some(e);
                        break 'outer;
                    }
                }
                if samples.len() >= target_len {
                    break 'outer;
                }
            }
        }

        // SAFETY: `pkt`/`frame` были выделены в начале этой функции и не
        // передавались никому за её пределы.
        unsafe {
            let mut pkt = pkt;
            av_packet_free(&mut pkt);
            let mut frame = frame;
            av_frame_free(&mut frame);
        }

        if let Some(e) = convert_err {
            return Err(e);
        }
        if samples.is_empty() {
            return Err("не удалось декодировать ни одного PCM-семпла (EOF или битый поток)".to_string());
        }
        Ok(samples)
    }

    /// Конвертирует декодированный аудио-`frame` в интерливленный PCM S16.
    /// Поддержаны только форматы из [`AV_SAMPLE_FMT_U8`]/`_S16`/`_FLT`
    /// (packed) и их planar-варианты (`_U8P`/`_S16P`/`_FLTP`) — этого
    /// достаточно для AAC/Opus/Vorbis, которые декодеры обычно отдают как
    /// `FLTP`. `S32`/`DBL`/`S64` и их planar-варианты — явная ошибка:
    /// добавлять их стоит вместе с живым `.mp4`/`.webm`, который реально
    /// их использует (тот же принцип, что и у остального FFI-слоя — не
    /// объявлять непроверенное живьём).
    ///
    /// # Safety
    /// `frame` — валидный, только что успешно декодированный `AVFrameHead`
    /// с `format == fmt` и `(*frame).nb_samples == nb_samples`; `channels`
    /// не превышает 8 (число слотов `AVFrameHead::data`) — проверено
    /// вызывающей стороной ([`Self::decode_audio_pcm`]).
    unsafe fn frame_to_interleaved_i16(
        frame: *mut AVFrameHead,
        fmt: c_int,
        nb_samples: usize,
        channels: usize,
    ) -> Result<Vec<i16>, String> {
        let mut out = vec![0i16; nb_samples * channels];
        match fmt {
            AV_SAMPLE_FMT_S16 => {
                // SAFETY: packed S16 — `data[0]` содержит `nb_samples *
                // channels` интерливленных `i16`, контракт функции.
                let src = unsafe { (*frame).data[0] }.cast::<i16>();
                for (i, slot) in out.iter_mut().enumerate() {
                    // SAFETY: `i < nb_samples * channels == out.len()`.
                    *slot = unsafe { *src.add(i) };
                }
            }
            AV_SAMPLE_FMT_S16P => {
                for ch in 0..channels {
                    // SAFETY: `data[ch]` — `ch < channels <= 8`, planar
                    // S16 буфер этого канала на `nb_samples` элементов.
                    let src = unsafe { (*frame).data[ch] }.cast::<i16>();
                    for i in 0..nb_samples {
                        // SAFETY: `i < nb_samples`, `src` — planar S16
                        // буфер этого канала ровно на `nb_samples`
                        // элементов (см. `SAFETY` над `src`).
                        out[i * channels + ch] = unsafe { *src.add(i) };
                    }
                }
            }
            AV_SAMPLE_FMT_FLT => {
                // SAFETY: packed float — `data[0]` содержит `nb_samples *
                // channels` интерливленных `f32` в диапазоне `[-1.0, 1.0]`.
                let src = unsafe { (*frame).data[0] }.cast::<f32>();
                for (i, slot) in out.iter_mut().enumerate() {
                    // SAFETY: `i < nb_samples * channels == out.len()`.
                    *slot = f32_sample_to_i16(unsafe { *src.add(i) });
                }
            }
            AV_SAMPLE_FMT_FLTP => {
                for ch in 0..channels {
                    // SAFETY: planar float — тот же аргумент, что и `S16P`.
                    let src = unsafe { (*frame).data[ch] }.cast::<f32>();
                    for i in 0..nb_samples {
                        // SAFETY: `i < nb_samples`, тот же аргумент, что и
                        // `S16P` выше.
                        out[i * channels + ch] = f32_sample_to_i16(unsafe { *src.add(i) });
                    }
                }
            }
            AV_SAMPLE_FMT_U8 => {
                // SAFETY: packed unsigned 8-bit, центр на 128.
                let src = unsafe { (*frame).data[0] };
                for (i, slot) in out.iter_mut().enumerate() {
                    // SAFETY: `i < nb_samples * channels == out.len()`.
                    *slot = u8_sample_to_i16(unsafe { *src.add(i) });
                }
            }
            AV_SAMPLE_FMT_U8P => {
                for ch in 0..channels {
                    // SAFETY: тот же аргумент, что и `S16P`, для U8 planar.
                    let src = unsafe { (*frame).data[ch] };
                    for i in 0..nb_samples {
                        // SAFETY: `i < nb_samples`, тот же аргумент, что и
                        // `S16P` выше.
                        out[i * channels + ch] = u8_sample_to_i16(unsafe { *src.add(i) });
                    }
                }
            }
            other => {
                return Err(format!(
                    "FFmpeg: неподдерживаемый формат сэмплов аудио (AVSampleFormat={other}) — нужен swresample или явная поддержка этого формата"
                ));
            }
        }
        Ok(out)
    }

    /// Декодирует кадры от текущей позиции чтения демуксера вперёд, пока
    /// pts декодированного кадра не достигнет `target_secs` (или пока не
    /// закончится поток — тогда возвращается последний декодированный
    /// кадр). Не делает seek сам — вызывающая сторона решает, нужен ли он.
    fn decode_from_current_position(&mut self, target_secs: f64) -> Result<(u32, u32, Vec<u8>), String> {
        // SAFETY: `av_packet_alloc` — обычная C-аллокация; null проверяется
        // сразу ниже перед любым использованием.
        let pkt = unsafe { av_packet_alloc() };
        // SAFETY: `av_frame_alloc` — обычная C-аллокация; null проверяется
        // сразу ниже перед любым использованием.
        let frame = unsafe { av_frame_alloc() };
        if pkt.is_null() || frame.is_null() {
            // SAFETY: `av_packet_free`/`av_frame_free` документированно
            // принимают указатель на null-переменную как no-op.
            unsafe {
                let mut pkt = pkt;
                av_packet_free(&mut pkt);
                let mut frame = frame;
                av_frame_free(&mut frame);
            }
            return Err("av_packet_alloc/av_frame_alloc вернул null".to_string());
        }

        let mut decoded: Option<(i64, c_int, c_int, c_int)> = None;
        loop {
            // SAFETY: `self.fmt_ctx` открыт и жив на весь срок жизни
            // `self`; `pkt` — валидный, только что выделенный `AVPacket`.
            let read_ret = unsafe { av_read_frame(self.fmt_ctx, pkt) };
            if read_ret < 0 {
                break;
            }
            // SAFETY: `pkt` заполнен успешным `av_read_frame` выше.
            let pkt_stream = unsafe { (*pkt).stream_index };
            if pkt_stream != self.stream_index {
                // SAFETY: `pkt` — тот же валидный пакет, `av_packet_unref`
                // — штатный способ освободить его данные без деалокации
                // самой структуры (она переиспользуется в цикле).
                unsafe {
                    av_packet_unref(pkt);
                }
                continue;
            }
            // SAFETY: `self.codec_ctx` открыт `avcodec_open2` в `open()`;
            // `pkt` содержит данные текущей видеодорожки.
            let send_ret = unsafe { avcodec_send_packet(self.codec_ctx, pkt) };
            // SAFETY: `pkt` больше не нужен после `avcodec_send_packet`
            // (FFmpeg копирует/референсит данные внутри), безопасно
            // освободить перед следующей итерацией.
            unsafe {
                av_packet_unref(pkt);
            }
            if send_ret < 0 {
                continue;
            }
            // SAFETY: `self.codec_ctx` открыт; `frame` — валидный,
            // выделенный выше `AVFrame`, переиспользуемый между попытками
            // приёма (FFmpeg перезаписывает его поля при успехе).
            let recv_ret = unsafe { avcodec_receive_frame(self.codec_ctx, frame) };
            if recv_ret != 0 {
                continue;
            }
            // SAFETY: `avcodec_receive_frame` вернул успех — поля `frame`
            // заполнены декодером по заявленному в `ffi.rs` layout'у.
            let (pts, w, h, fmt) = unsafe { ((*frame).pts, (*frame).width, (*frame).height, (*frame).format) };
            let pts_secs = if self.time_base_den == 0 {
                0.0
            } else {
                pts as f64 * f64::from(self.time_base_num) / f64::from(self.time_base_den)
            };
            decoded = Some((pts, w, h, fmt));
            if pts_secs + 0.001 >= target_secs {
                break;
            }
        }

        let result = match decoded {
            None => Err("не удалось декодировать ни одного кадра".to_string()),
            Some((_pts, w, h, fmt)) => {
                if w <= 0 || h <= 0 {
                    Err(format!("декодер вернул некорректные размеры кадра {w}x{h}"))
                } else {
                    // SAFETY: `sws_getContext`/`sws_scale` вызываются с
                    // исходными размерами/форматом только что успешно
                    // декодированного `frame`; `frame.data`/`linesize`
                    // заполнены `avcodec_receive_frame` выше.
                    unsafe { self.scale_frame_to_rgba8(frame, w, h, fmt) }
                }
            }
        };

        // SAFETY: `pkt`/`frame` были выделены в начале этой функции и не
        // передавались никому за её пределы.
        unsafe {
            let mut pkt = pkt;
            av_packet_free(&mut pkt);
            let mut frame = frame;
            av_frame_free(&mut frame);
        }

        result
    }

    /// # Safety
    /// `frame` — валидный, только что успешно декодированный `AVFrameHead`
    /// с `width == w`, `height == h`, `format == fmt`.
    unsafe fn scale_frame_to_rgba8(
        &self,
        frame: *mut AVFrameHead,
        w: c_int,
        h: c_int,
        fmt: c_int,
    ) -> Result<(u32, u32, Vec<u8>), String> {
        // SAFETY: контракт этой функции (см. `# Safety` выше) гарантирует
        // валидность `w`/`h`/`fmt` для этого `frame`.
        let sws = unsafe {
            sws_getContext(
                w,
                h,
                fmt,
                w,
                h,
                AV_PIX_FMT_RGBA,
                SWS_BILINEAR,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if sws.is_null() {
            return Err("sws_getContext вернул null (неподдерживаемый исходный формат пикселей)".to_string());
        }

        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        let dst_stride = [w * 4, 0, 0, 0, 0, 0, 0, 0];
        let dst_slice: [*mut u8; 8] = [
            rgba.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        ];
        // SAFETY: `frame` валиден по контракту функции; `rgba` только что
        // выделен ровно под `w*h*4` байт, `dst_stride[0] = w*4` совпадает
        // с реальным шагом строки в `rgba`.
        unsafe {
            let src_slice: [*const u8; 8] = [
                (*frame).data[0],
                (*frame).data[1],
                (*frame).data[2],
                (*frame).data[3],
                (*frame).data[4],
                (*frame).data[5],
                (*frame).data[6],
                (*frame).data[7],
            ];
            let src_stride = (*frame).linesize;
            sws_scale(
                sws,
                src_slice.as_ptr(),
                src_stride.as_ptr(),
                0,
                h,
                dst_slice.as_ptr(),
                dst_stride.as_ptr(),
            );
        }
        // SAFETY: `sws` не использовался ни в одном другом потоке
        // управления и не нужен после `sws_scale` выше.
        unsafe {
            sws_freeContext(sws);
        }

        Ok((w as u32, h as u32, rgba))
    }
}

impl VideoDecodeSession for FfmpegSession {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn duration_secs(&self) -> Option<f64> {
        self.duration_secs
    }

    fn frame_at(&mut self, secs: f64) -> Result<Vec<u8>, String> {
        if (self.first_frame.0 - secs).abs() < 0.001 {
            return Ok(self.first_frame.1.clone());
        }

        if self.time_base_den == 0 {
            return Err("FFmpeg: поток с нулевым time_base — перемотка невозможна".to_string());
        }
        let target_units = (secs * f64::from(self.time_base_den) / f64::from(self.time_base_num)) as i64;
        // SAFETY: `self.fmt_ctx` открыт и жив на весь срок жизни `self`;
        // `self.stream_index` в границах потоков контейнера (проверено при
        // `open()`).
        let seek_ret = unsafe {
            av_seek_frame(self.fmt_ctx, self.stream_index, target_units, AVSEEK_FLAG_BACKWARD)
        };
        if seek_ret < 0 {
            return Err(format!("FFmpeg: av_seek_frame: {}", describe_error(seek_ret)));
        }
        // SAFETY: `self.codec_ctx` открыт; сброс внутреннего буфера
        // декодера обязателен после seek демуксера — иначе первые кадры
        // после перемотки декодируются с состоянием (референсные кадры) от
        // позиции до seek.
        unsafe {
            avcodec_flush_buffers(self.codec_ctx);
        }

        let (_w, _h, rgba) = self.decode_from_current_position(secs)?;
        Ok(rgba)
    }

    fn audio_track(&self) -> Option<AudioTrackInfo> {
        self.audio_track
    }

    fn decode_audio_pcm(&mut self, max_samples: usize) -> Result<Vec<i16>, String> {
        Self::decode_audio_pcm(self, max_samples)
    }
}

impl Drop for FfmpegSession {
    fn drop(&mut self) {
        // SAFETY: `self.codec_ctx`/`self.audio_codec_ctx`/`self.fmt_ctx`/
        // `self.avio_ctx`/`self.reader` были созданы вместе в `open()` и с
        // тех пор ничем не переиспользованы — это единственный `Drop`,
        // освобождение в порядке кодек(и) → демуксер → custom IO → буфер
        // байт зеркалирует порядок создания в обратную сторону, как и в
        // `open()`'s cleanup веток отказа. `avcodec_free_context` безопасно
        // принимает указатель на уже-null `audio_codec_ctx` (контейнер без
        // декодируемой аудиодорожки, срез 12/13) как no-op.
        unsafe {
            avcodec_free_context(&mut self.codec_ctx);
            avcodec_free_context(&mut self.audio_codec_ctx);
            avformat_close_input(&mut self.fmt_ctx);
            // `AVFMT_FLAG_CUSTOM_IO` — `avformat_close_input` не трогает
            // `avio_ctx`, освобождаем сами.
            avio_context_free(&mut self.avio_ctx);
            drop(Box::from_raw(self.reader));
        }
    }
}
