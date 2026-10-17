//! H.264 / HEVC через `VTDecompressionSession`: пакеты MP4 (с префиксами длины,
//! как лежат в файле) → VideoToolbox → NV12 (P010 для 10 бит) → RGBA.
//!
//! Формат потока задаёт `CMVideoFormatDescription` с атомом `avcC`/`hvcC` из
//! контейнера — тем же, что у QuickTime, поэтому разбирать SPS/PPS самим не нужно.
//! Кадры выходят из сессии в порядке показа (`EnableTemporalProcessing`); обратный
//! вызов копирует плоскости пиксельного буфера в собственную память, так что
//! буферы VideoToolbox сразу возвращаются в пул.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use lumen_media_demux::{Codec, Packet, TrackInfo};

use super::ffi::{self, CMTime, CVPixelBufferRef, VTDecompressionSessionRef};
use super::{check, software_only, Cf};
use crate::registry::{DecodedFrame, VideoFrameDecoder};
use crate::yuv::{Layout, Matrix, SemiplanarFrame};

/// Кадры, выданные обратным вызовом и ещё не забранные декодером.
struct Shared {
    frames: Mutex<Vec<SemiplanarFrame>>,
}

/// Ошибки VideoToolbox, после которых воспроизведение продолжается со следующего
/// кадра: повреждённые данные и потерянные опорные кадры (после перемотки в
/// середину GOP).
fn is_skippable(status: i32) -> bool {
    matches!(status, -8969 | -12909 | -17694)
}

/// Декодер одной видеодорожки.
pub(super) struct VtVideoDecoder {
    format: Cf,
    session: VTDecompressionSessionRef,
    shared: Arc<Shared>,
    ready: VecDeque<Box<dyn DecodedFrame>>,
}

// SAFETY: сессия VideoToolbox потокобезопасна, а декодер используется только через
// `&mut self`; общее состояние обратного вызова защищено `Mutex`.
unsafe impl Send for VtVideoDecoder {}

impl VtVideoDecoder {
    pub(super) fn new(track: &TrackInfo) -> Result<Self, String> {
        let (codec_type, atom) = match track.codec {
            Codec::H264 => (ffi::kCMVideoCodecType_H264, "avcC"),
            Codec::Hevc => (ffi::kCMVideoCodecType_HEVC, "hvcC"),
            other => return Err(format!("VideoToolbox: видеокодек {other:?} не поддерживается")),
        };
        let format = create_format(track, codec_type, atom)?;
        let shared = Arc::new(Shared { frames: Mutex::new(Vec::new()) });
        let session = create_session(track, &format, &shared)?;
        let mode = if software_only() { "программный" } else { "аппаратный, если есть" };
        eprintln!("[media] видео {:?}: VideoToolbox ({mode})", track.codec);
        Ok(Self { format, session, shared, ready: VecDeque::new() })
    }

    /// Забрать кадры, которые сессия успела выдать, в порядке показа.
    fn collect(&mut self) {
        let mut batch = match self.shared.frames.lock() {
            Ok(mut q) => std::mem::take(&mut *q),
            Err(_) => return,
        };
        batch.sort_by_key(|f| f.pts_us);
        self.ready.extend(batch.into_iter().map(|f| Box::new(f) as Box<dyn DecodedFrame>));
    }

    fn decode(&mut self, packet: &Packet) -> Result<(), String> {
        let len = packet.data.len();
        if len == 0 {
            return Ok(());
        }
        let mut block = std::ptr::null();
        // SAFETY: память блока выделяет CoreMedia (`memory_block` пуст, флаг требует
        // выделить её сразу); `block` — выходной параметр.
        let status = unsafe {
            ffi::CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null(),
                std::ptr::null_mut(),
                len,
                std::ptr::null(),
                std::ptr::null(),
                0,
                len,
                ffi::kCMBlockBufferAssureMemoryNowFlag,
                &raw mut block,
            )
        };
        check("CMBlockBufferCreateWithMemoryBlock", status)?;
        let block = Cf::new(block).ok_or("CMBlockBuffer не создан")?;
        // SAFETY: блок выделен на `len` байт; источник — `len` байт пакета.
        let status = unsafe {
            ffi::CMBlockBufferReplaceDataBytes(packet.data.as_ptr().cast::<c_void>(), block.raw(), 0, len)
        };
        check("CMBlockBufferReplaceDataBytes", status)?;

        let timing = ffi::CMSampleTimingInfo {
            duration: CMTime::from_micros(packet.duration_us.max(0)),
            presentationTimeStamp: CMTime::from_micros(packet.pts_us),
            decodeTimeStamp: CMTime::INVALID,
        };
        let mut sample = std::ptr::null();
        // SAFETY: один образец с одной записью времени и одним размером; блок и
        // формат живы; `sample` — выходной параметр.
        let status = unsafe {
            ffi::CMSampleBufferCreate(
                std::ptr::null(),
                block.raw(),
                1,
                std::ptr::null(),
                std::ptr::null_mut(),
                self.format.raw(),
                1,
                1,
                &raw const timing,
                1,
                &raw const len,
                &raw mut sample,
            )
        };
        check("CMSampleBufferCreate", status)?;
        let sample = Cf::new(sample).ok_or("CMSampleBuffer не создан")?;

        let mut info = 0u32;
        // SAFETY: сессия жива; обратный вызов вызывается синхронно или из потока
        // VideoToolbox и обращается только к `Shared`.
        let status = unsafe {
            ffi::VTDecompressionSessionDecodeFrame(
                self.session,
                sample.raw(),
                ffi::kVTDecodeFrame_EnableTemporalProcessing,
                std::ptr::null_mut(),
                &raw mut info,
            )
        };
        if is_skippable(status) {
            return Ok(());
        }
        check("VTDecompressionSessionDecodeFrame", status)
    }

    /// Вытолкнуть кадры, задержанные ради переупорядочивания.
    fn finish(&mut self) -> Result<(), String> {
        // SAFETY: сессия жива.
        let status = unsafe { ffi::VTDecompressionSessionFinishDelayedFrames(self.session) };
        check("VTDecompressionSessionFinishDelayedFrames", status)?;
        // SAFETY: сессия жива.
        let status = unsafe { ffi::VTDecompressionSessionWaitForAsynchronousFrames(self.session) };
        check("VTDecompressionSessionWaitForAsynchronousFrames", status)
    }
}

impl Drop for VtVideoDecoder {
    fn drop(&mut self) {
        // SAFETY: сессию создала `VTDecompressionSessionCreate` (одна ссылка у нас);
        // после `Invalidate` обратные вызовы больше не приходят, и `Shared` можно освободить.
        unsafe {
            ffi::VTDecompressionSessionInvalidate(self.session);
            ffi::CFRelease(self.session.cast_const());
        }
    }
}

fn create_format(track: &TrackInfo, codec_type: u32, atom: &str) -> Result<Cf, String> {
    let (width, height) = (
        i32::try_from(track.width).map_err(|_| "ширина кадра вне диапазона")?,
        i32::try_from(track.height).map_err(|_| "высота кадра вне диапазона")?,
    );
    let key = Cf::string(atom).ok_or("CFString не создана")?;
    let config = Cf::data(&track.extradata).ok_or("CFData не создана")?;
    let atoms = Cf::dictionary(&[(key.raw(), config.raw())]).ok_or("CFDictionary не создан")?;
    // SAFETY: чтение глобальной константы CoreMedia.
    let extensions_key = unsafe { ffi::kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms };
    let extensions = Cf::dictionary(&[(extensions_key, atoms.raw())]).ok_or("CFDictionary не создан")?;
    let mut format = std::ptr::null();
    // SAFETY: расширения живы до конца вызова (формат берёт собственную копию);
    // `format` — выходной параметр.
    let status = unsafe {
        ffi::CMVideoFormatDescriptionCreate(
            std::ptr::null(),
            codec_type,
            width,
            height,
            extensions.raw(),
            &raw mut format,
        )
    };
    check("CMVideoFormatDescriptionCreate", status)?;
    Cf::new(format).ok_or_else(|| "CMVideoFormatDescription не создан".to_string())
}

fn create_session(track: &TrackInfo, format: &Cf, shared: &Arc<Shared>) -> Result<VTDecompressionSessionRef, String> {
    let ten_bit = track.codec == Codec::Hevc && track.codec_string.contains(".2.");
    let pixel_format = if ten_bit {
        ffi::kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
    } else {
        ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
    };
    let number = Cf::number(pixel_format as i32).ok_or("CFNumber не создан")?;
    // SAFETY: чтение глобальной константы CoreVideo.
    let pixel_key = unsafe { ffi::kCVPixelBufferPixelFormatTypeKey };
    let attributes = Cf::dictionary(&[(pixel_key, number.raw())]).ok_or("CFDictionary не создан")?;
    // SAFETY: чтение глобальных констант CoreFoundation и VideoToolbox.
    let (hardware_key, hardware) = unsafe {
        (
            ffi::kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
            if software_only() { ffi::kCFBooleanFalse } else { ffi::kCFBooleanTrue },
        )
    };
    let specification = Cf::dictionary(&[(hardware_key, hardware)]).ok_or("CFDictionary не создан")?;
    let callback = ffi::VTDecompressionOutputCallbackRecord {
        decompressionOutputCallback: Some(output_callback),
        decompressionOutputRefCon: Arc::as_ptr(shared).cast_mut().cast::<c_void>(),
    };
    let mut session: VTDecompressionSessionRef = std::ptr::null_mut();
    // SAFETY: формат и словари живы на время вызова (сессия удерживает свои копии);
    // `shared` переживает сессию — `Drop` декодера сначала инвалидирует её.
    let status = unsafe {
        ffi::VTDecompressionSessionCreate(
            std::ptr::null(),
            format.raw(),
            specification.raw(),
            attributes.raw(),
            &raw const callback,
            &raw mut session,
        )
    };
    check("VTDecompressionSessionCreate", status)?;
    if session.is_null() {
        return Err("VTDecompressionSession не создана".to_string());
    }
    Ok(session)
}

/// Обратный вызов сессии: готовый кадр → копия в `Shared`.
unsafe extern "C" fn output_callback(
    refcon: *mut c_void,
    _source_frame_refcon: *mut c_void,
    status: i32,
    _info_flags: u32,
    image: CVPixelBufferRef,
    presentation_time_stamp: CMTime,
    _presentation_duration: CMTime,
) {
    if status != 0 || image.is_null() || refcon.is_null() {
        return;
    }
    let Some(pts_us) = presentation_time_stamp.micros() else { return };
    // SAFETY: `refcon` — `Shared`, который декодер держит живым до `Invalidate`.
    let shared = unsafe { &*refcon.cast::<Shared>() };
    if let Some(frame) = copy_frame(image, pts_us)
        && let Ok(mut queue) = shared.frames.lock()
    {
        queue.push(frame);
    }
}

/// Плоскость `plane` пиксельного буфера → `rows` строк по `row_bytes` в `dst` с
/// шагом `dst_stride`. `None` — буфер короче, чем нужно.
///
/// # Safety
/// Буфер заблокирован (`CVPixelBufferLockBaseAddress`) и жив.
unsafe fn copy_plane(
    image: CVPixelBufferRef,
    plane: usize,
    rows: usize,
    row_bytes: usize,
    dst: &mut [u8],
    dst_stride: usize,
) -> Option<()> {
    // SAFETY: по контракту буфер заблокирован.
    let (src, src_stride) = unsafe {
        (ffi::CVPixelBufferGetBaseAddressOfPlane(image, plane), ffi::CVPixelBufferGetBytesPerRowOfPlane(image, plane))
    };
    if src.is_null() || src_stride < row_bytes {
        return None;
    }
    for row in 0..rows {
        let line = dst.get_mut(row * dst_stride..row * dst_stride + row_bytes)?;
        // SAFETY: плоскость хранит `rows` строк по `src_stride` байт, из них
        // `row_bytes` нужные; `line` — ровно `row_bytes` байт приёмника.
        unsafe { std::ptr::copy_nonoverlapping(src.add(row * src_stride), line.as_mut_ptr(), row_bytes) };
    }
    Some(())
}

/// Пиксельный буфер (NV12 / P010) → собственный кадр с общим шагом строк.
fn copy_frame(image: CVPixelBufferRef, pts_us: i64) -> Option<SemiplanarFrame> {
    // SAFETY: `image` — живой пиксельный буфер из обратного вызова.
    let (format, planes, width, height) = unsafe {
        (
            ffi::CVPixelBufferGetPixelFormatType(image),
            ffi::CVPixelBufferGetPlaneCount(image),
            ffi::CVPixelBufferGetWidth(image),
            ffi::CVPixelBufferGetHeight(image),
        )
    };
    let (bytes_per_sample, full_range) = match format {
        ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange => (1, false),
        ffi::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange => (1, true),
        ffi::kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange => (2, false),
        ffi::kCVPixelFormatType_420YpCbCr10BiPlanarFullRange => (2, true),
        _ => return None,
    };
    if planes != 2 || width == 0 || height == 0 {
        return None;
    }
    let stride = width.div_ceil(2) * 2 * bytes_per_sample;
    let chroma_rows = height.div_ceil(2);
    let mut data = vec![0u8; stride * (height + chroma_rows)];
    // SAFETY: блокировка парная с разблокировкой ниже.
    if unsafe { ffi::CVPixelBufferLockBaseAddress(image, ffi::kCVPixelBufferLock_ReadOnly) } != 0 {
        return None;
    }
    let (luma, chroma) = data.split_at_mut(stride * height);
    // SAFETY: буфер заблокирован выше. В яркостной плоскости строка — `width`
    // сэмплов, в цветовой — `ceil(width / 2)` пар.
    let copied = unsafe {
        copy_plane(image, 0, height, width * bytes_per_sample, luma, stride)
            .and_then(|()| copy_plane(image, 1, chroma_rows, stride, chroma, stride))
    };
    // SAFETY: парная разблокировка.
    unsafe { ffi::CVPixelBufferUnlockBaseAddress(image, ffi::kCVPixelBufferLock_ReadOnly) };
    copied?;

    // SAFETY: чтение вложения и констант CoreVideo; `attachment` принадлежит буферу
    // и живёт, пока жив он.
    let matrix = unsafe {
        let attachment = ffi::CVBufferGetAttachment(image, ffi::kCVImageBufferYCbCrMatrixKey, std::ptr::null_mut());
        if attachment.is_null() {
            Matrix::guess(height as u32)
        } else if ffi::CFEqual(attachment, ffi::kCVImageBufferYCbCrMatrix_ITU_R_709_2) != 0 {
            Matrix::Bt709
        } else {
            Matrix::Bt601
        }
    };
    let layout = Layout {
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
        stride,
        uv_offset: stride * height,
        bytes_per_sample,
        matrix,
        full_range,
    };
    Some(SemiplanarFrame { pts_us, layout, data })
}

impl VideoFrameDecoder for VtVideoDecoder {
    fn send(&mut self, packet: &Packet, _restart: bool) -> Result<(), String> {
        let result = self.decode(packet);
        self.collect();
        result
    }

    fn next_frame(&mut self) -> Option<Box<dyn DecodedFrame>> {
        self.ready.pop_front()
    }

    fn drain(&mut self) -> Result<(), String> {
        let result = self.finish();
        self.collect();
        result
    }

    fn flush(&mut self) {
        // Кадры, оставшиеся от прежней позиции, выходят здесь и отбрасываются.
        let _ = self.finish();
        if let Ok(mut queue) = self.shared.frames.lock() {
            queue.clear();
        }
        self.ready.clear();
    }
}
