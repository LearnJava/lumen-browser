//! H.264 / HEVC через MFT-декодер ОС: пакеты MP4 → Annex B → `IMFTransform` →
//! NV12 (P010 для 10 бит) → RGBA.

use std::collections::VecDeque;
use std::mem::ManuallyDrop;

use lumen_media_demux::{Codec, Packet, TrackInfo};
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    IMFMediaType, IMFSample, IMFTransform, MFMediaType_Video, MFSampleExtension_CleanPoint,
    MFSampleExtension_Discontinuity, MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_NV12,
    MFVideoFormat_P010, MFT_CATEGORY_VIDEO_DECODER, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_DATA_BUFFER_FORMAT_CHANGE, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MF_E_NOTACCEPTING,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_YUV_MATRIX,
};

use super::{ensure_com, Api};
use crate::annexb::{self, ParameterSets};
use crate::registry::{DecodedFrame, VideoFrameDecoder};
use crate::yuv::{self, Layout, Matrix};

/// `MFVideoInterlace_Progressive`.
const INTERLACE_PROGRESSIVE: u32 = 2;
/// `MFVideoTransferMatrix_BT709` / `_BT601`.
const MATRIX_BT709: u32 = 1;
const MATRIX_BT601: u32 = 2;
/// `MFNominalRange_0_255`.
const RANGE_FULL: u32 = 1;
/// Запас на размер выходного буфера, если MFT не назвал свой.
const MIN_OUTPUT_BYTES: u32 = 1 << 20;

/// Формат выходных кадров, как его описывает текущий выходной тип MFT.
#[derive(Clone, Copy)]
struct OutputFormat {
    /// Размер буфера кадра (кодированный, с выравниванием).
    coded_height: u32,
    /// Видимая область.
    width: u32,
    height: u32,
    stride: usize,
    bytes_per_sample: usize,
    matrix: Matrix,
    full_range: bool,
}

pub(super) struct MfVideoDecoder {
    api: &'static Api,
    mft: IMFTransform,
    params: ParameterSets,
    out: Option<OutputFormat>,
    /// MFT сам выделяет выходные образцы (иначе — выделяем мы).
    provides_samples: bool,
    out_bytes: u32,
    ready: VecDeque<Box<dyn DecodedFrame>>,
    /// Метка последнего выданного кадра, мкс: запасная для кадров без метки.
    last_pts_us: i64,
}

// SAFETY: MFT ОС «free-threaded» (модель потоков Both), а сессия обращается к
// декодеру только из одного потока за раз (`&mut self`); образцы наружу не
// выходят — кадры копируются в `Vec<u8>`.
unsafe impl Send for MfVideoDecoder {}

fn hr(what: &str, e: windows::core::Error) -> String {
    format!("{what}: {e}")
}

impl MfVideoDecoder {
    pub(super) fn new(track: &TrackInfo) -> Result<Self, String> {
        ensure_com();
        let api = Api::get().ok_or("Media Foundation недоступна")?;
        let (subtype, params) = match track.codec {
            Codec::H264 => (MFVideoFormat_H264, annexb::parse_avcc(&track.extradata)?),
            Codec::Hevc => (MFVideoFormat_HEVC, annexb::parse_hvcc(&track.extradata)?),
            other => return Err(format!("Media Foundation: видеокодек {other:?} не поддерживается")),
        };
        let mft: IMFTransform = api.create_transform(MFT_CATEGORY_VIDEO_DECODER, MFMediaType_Video, subtype)?;

        let input = api.new_media_type()?;
        // SAFETY: тип создан выше; ключи — константы Media Foundation.
        unsafe {
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| hr("MAJOR_TYPE", e))?;
            input.SetGUID(&MF_MT_SUBTYPE, &subtype).map_err(|e| hr("SUBTYPE", e))?;
            let size = (u64::from(track.width) << 32) | u64::from(track.height);
            input.SetUINT64(&MF_MT_FRAME_SIZE, size).map_err(|e| hr("FRAME_SIZE", e))?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, INTERLACE_PROGRESSIVE).map_err(|e| hr("INTERLACE_MODE", e))?;
            mft.SetInputType(0, &input, 0).map_err(|e| hr("SetInputType", e))?;
        }
        let mut dec = Self {
            api,
            mft,
            params,
            out: None,
            provides_samples: false,
            out_bytes: MIN_OUTPUT_BYTES,
            ready: VecDeque::new(),
            last_pts_us: 0,
        };
        // Выходной тип выбирается заранее, если MFT уже готов его назвать; иначе
        // он придёт с `MF_E_TRANSFORM_STREAM_CHANGE` на первом кадре.
        let ten_bit = track.codec == Codec::Hevc && track.codec_string.contains(".2.");
        let _ = dec.select_output(ten_bit);
        // SAFETY: сообщения потоковой модели MFT.
        unsafe {
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).map_err(|e| hr("BEGIN_STREAMING", e))?;
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).map_err(|e| hr("START_OF_STREAM", e))?;
        }
        Ok(dec)
    }

    /// Выбрать из предложенных MFT выходных типов NV12 (для 10 бит — P010) и
    /// прочитать получившийся формат.
    fn select_output(&mut self, prefer_p010: bool) -> Result<(), String> {
        let wanted = if prefer_p010 { [MFVideoFormat_P010, MFVideoFormat_NV12] } else { [MFVideoFormat_NV12, MFVideoFormat_P010] };
        let mut types: Vec<(GUID, IMFMediaType)> = Vec::new();
        for i in 0.. {
            // SAFETY: перебор типов, пока MFT не ответит `MF_E_NO_MORE_TYPES`.
            let Ok(t) = (unsafe { self.mft.GetOutputAvailableType(0, i) }) else { break };
            // SAFETY: чтение атрибута типа.
            if let Ok(sub) = unsafe { t.GetGUID(&MF_MT_SUBTYPE) } {
                types.push((sub, t));
            }
        }
        let chosen = wanted
            .iter()
            .find_map(|w| types.iter().find(|(s, _)| s == w))
            .ok_or("декодер не предлагает NV12/P010")?;
        // SAFETY: тип получен от этого же MFT.
        unsafe { self.mft.SetOutputType(0, &chosen.1, 0) }.map_err(|e| hr("SetOutputType", e))?;
        self.refresh_output()
    }

    /// Прочитать текущий выходной тип и параметры выходного потока.
    fn refresh_output(&mut self) -> Result<(), String> {
        // SAFETY: запросы к MFT с уже выбранным выходным типом.
        let (t, info) = unsafe {
            (
                self.mft.GetOutputCurrentType(0).map_err(|e| hr("GetOutputCurrentType", e))?,
                self.mft.GetOutputStreamInfo(0).map_err(|e| hr("GetOutputStreamInfo", e))?,
            )
        };
        self.provides_samples = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
        self.out_bytes = info.cbSize.max(MIN_OUTPUT_BYTES);
        self.out = Some(read_format(&t)?);
        Ok(())
    }

    fn make_input_sample(&self, annexb: &[u8], packet: &Packet, restart: bool) -> Result<IMFSample, String> {
        let len = u32::try_from(annexb.len()).map_err(|_| "пакет больше 4 ГиБ".to_string())?;
        let buffer = self.api.new_buffer(len)?;
        // SAFETY: буфер создан на `len` байт; Lock/Unlock парные.
        unsafe {
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None).map_err(|e| hr("Lock", e))?;
            std::ptr::copy_nonoverlapping(annexb.as_ptr(), ptr, annexb.len());
            buffer.Unlock().map_err(|e| hr("Unlock", e))?;
            buffer.SetCurrentLength(len).map_err(|e| hr("SetCurrentLength", e))?;
        }
        let sample = self.api.new_sample()?;
        // SAFETY: образец создан выше; время — в единицах по 100 нс.
        unsafe {
            sample.AddBuffer(&buffer).map_err(|e| hr("AddBuffer", e))?;
            sample.SetSampleTime(packet.pts_us.saturating_mul(10)).map_err(|e| hr("SetSampleTime", e))?;
            sample.SetSampleDuration(packet.duration_us.max(0).saturating_mul(10)).map_err(|e| hr("SetSampleDuration", e))?;
            if packet.keyframe {
                let _ = sample.SetUINT32(&MFSampleExtension_CleanPoint, 1);
            }
            if restart {
                let _ = sample.SetUINT32(&MFSampleExtension_Discontinuity, 1);
            }
        }
        Ok(sample)
    }

    /// Забрать все готовые кадры; возвращается, когда MFT просит новых данных.
    fn pull_outputs(&mut self) -> Result<(), String> {
        loop {
            let provided = if self.provides_samples {
                None
            } else {
                let sample = self.api.new_sample()?;
                let buffer = self.api.new_buffer(self.out_bytes)?;
                // SAFETY: свежие объекты.
                unsafe { sample.AddBuffer(&buffer) }.map_err(|e| hr("AddBuffer", e))?;
                Some(sample)
            };
            let mut data = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(provided),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            // SAFETY: один выходной буфер; поля `pSample`/`pEvents` забираем ниже.
            let result = unsafe { self.mft.ProcessOutput(0, &mut data, &mut status) };
            // SAFETY: после вызова поля принадлежат нам — снимаем `ManuallyDrop`.
            let (sample, flags) = unsafe {
                let sample = ManuallyDrop::take(&mut data[0].pSample);
                drop(ManuallyDrop::take(&mut data[0].pEvents));
                (sample, data[0].dwStatus)
            };
            match result {
                Ok(()) => {
                    if flags & (MFT_OUTPUT_DATA_BUFFER_FORMAT_CHANGE.0 as u32) != 0 {
                        self.refresh_output()?;
                    }
                    if let Some(sample) = sample {
                        self.take_frame(&sample)?;
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.select_output(false)?;
                }
                Err(e) => return Err(hr("ProcessOutput", e)),
            }
        }
    }

    /// Скопировать образец MFT в собственный кадр (MFT переиспользует буферы).
    fn take_frame(&mut self, sample: &IMFSample) -> Result<(), String> {
        let fmt = self.out.ok_or("кадр до выбора выходного типа")?;
        // SAFETY: образец от MFT; Lock/Unlock парные, срез не переживает Unlock.
        let (bytes, pts_us) = unsafe {
            let buffer = sample.ConvertToContiguousBuffer().map_err(|e| hr("ConvertToContiguousBuffer", e))?;
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut ptr, None, Some(&mut len)).map_err(|e| hr("Lock", e))?;
            let bytes = std::slice::from_raw_parts(ptr, len as usize).to_vec();
            buffer.Unlock().map_err(|e| hr("Unlock", e))?;
            let pts_us = sample.GetSampleTime().map_or(self.last_pts_us, |t| t / 10);
            (bytes, pts_us)
        };
        self.last_pts_us = pts_us;
        // Высота буфера (с выравниванием по 16/32) — из его реальной длины.
        let buf_rows = bytes.len().saturating_mul(2) / (3 * fmt.stride.max(1));
        let rows = buf_rows.max(fmt.coded_height as usize);
        self.ready.push_back(Box::new(MfFrame {
            pts_us,
            layout: Layout {
                width: fmt.width,
                height: fmt.height,
                stride: fmt.stride,
                uv_offset: fmt.stride * rows,
                bytes_per_sample: fmt.bytes_per_sample,
                matrix: fmt.matrix,
                full_range: fmt.full_range,
            },
            data: bytes,
        }));
        Ok(())
    }
}

/// Размеры, шаг строки и цветовые параметры выходного типа.
fn read_format(t: &IMFMediaType) -> Result<OutputFormat, String> {
    // SAFETY: чтение атрибутов типа.
    unsafe {
        let size = t.GetUINT64(&MF_MT_FRAME_SIZE).map_err(|e| hr("FRAME_SIZE", e))?;
        let (coded_w, coded_h) = ((size >> 32) as u32, size as u32);
        let subtype = t.GetGUID(&MF_MT_SUBTYPE).map_err(|e| hr("SUBTYPE", e))?;
        let bytes_per_sample = if subtype == MFVideoFormat_P010 { 2 } else { 1 };

        // Видимая область: `MFVideoArea` = {OffsetX, OffsetY: {u16 fract, i16 value}, Area: {i32 cx, cy}}.
        let mut area = [0u8; 16];
        let (mut width, mut height) = (coded_w, coded_h);
        if t.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None).is_ok() {
            let cx = i32::from_le_bytes([area[8], area[9], area[10], area[11]]);
            let cy = i32::from_le_bytes([area[12], area[13], area[14], area[15]]);
            if cx > 0 && cy > 0 && cx as u32 <= coded_w && cy as u32 <= coded_h {
                (width, height) = (cx as u32, cy as u32);
            }
        }
        let stride = t
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map(|s| (s as i32).unsigned_abs() as usize)
            .ok()
            .filter(|&s| s > 0)
            .unwrap_or(coded_w as usize * bytes_per_sample);
        let matrix = match t.GetUINT32(&MF_MT_YUV_MATRIX) {
            Ok(MATRIX_BT709) => Matrix::Bt709,
            Ok(MATRIX_BT601) => Matrix::Bt601,
            _ => Matrix::guess(height),
        };
        let full_range = t.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE) == Ok(RANGE_FULL);
        Ok(OutputFormat { coded_height: coded_h, width, height, stride, bytes_per_sample, matrix, full_range })
    }
}

impl VideoFrameDecoder for MfVideoDecoder {
    fn send(&mut self, packet: &Packet, restart: bool) -> Result<(), String> {
        let prefix: &[u8] = if restart || packet.keyframe { &self.params.annexb } else { &[] };
        let annexb = annexb::to_annexb(&packet.data, self.params.nal_length_size, prefix)?;
        let sample = self.make_input_sample(&annexb, packet, restart)?;
        // SAFETY: образец готов; при `MF_E_NOTACCEPTING` MFT сначала опустошаем.
        let mut result = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        if result.as_ref().is_err_and(|e| e.code() == MF_E_NOTACCEPTING) {
            self.pull_outputs()?;
            // SAFETY: повтор после опустошения.
            result = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        }
        result.map_err(|e| hr("ProcessInput", e))?;
        self.pull_outputs()
    }

    fn next_frame(&mut self) -> Option<Box<dyn DecodedFrame>> {
        self.ready.pop_front()
    }

    fn drain(&mut self) -> Result<(), String> {
        // SAFETY: конец потока — вытолкнуть кадры из конвейера MFT.
        unsafe { self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }.map_err(|e| hr("DRAIN", e))?;
        self.pull_outputs()
    }

    fn flush(&mut self) {
        self.ready.clear();
        // SAFETY: перемотка — сбросить накопленное, затем начать поток заново.
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }
    }
}

/// Кадр, скопированный из образца MFT.
struct MfFrame {
    pts_us: i64,
    layout: Layout,
    data: Vec<u8>,
}

impl DecodedFrame for MfFrame {
    fn pts_us(&self) -> i64 {
        self.pts_us
    }

    fn width(&self) -> u32 {
        self.layout.width
    }

    fn height(&self) -> u32 {
        self.layout.height
    }

    fn to_rgba8(&self) -> Result<Vec<u8>, String> {
        yuv::semiplanar_to_rgba(&self.data, &self.layout)
    }
}
