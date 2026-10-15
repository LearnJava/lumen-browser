//! H.264 / HEVC через MFT-декодер ОС: пакеты MP4 → Annex B → `IMFTransform` →
//! NV12 (P010 для 10 бит) → RGBA.
//!
//! **Аппаратный путь (D3D11VA).** Тот же MFT получает D3D-менеджер
//! (`MFT_MESSAGE_SET_D3D_MANAGER`) и декодирует в текстуры GPU, откуда кадр
//! копируется в память ([`super::d3d`]). [`MfVideoDecoder`] сначала пробует этот
//! путь, а при любом сбое — инициализации или первых кадров — перезапускает
//! декодирование программным MFT с тех же пакетов: воспроизведение не прерывается.
//! `LUMEN_OS_DECODERS=sw` и блок-лист адаптеров ([`super::blocklist`]) отключают
//! аппаратный путь заранее.

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
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_YUV_MATRIX, MF_SA_D3D11_AWARE, MFT_MESSAGE_SET_D3D_MANAGER,
};

use super::d3d::Gpu;
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

/// Сколько пакетов аппаратный декодер может принять, не выдав ни одного кадра,
/// прежде чем его признают нерабочим (с запасом на очередь B-кадров).
const PROBATION_PACKETS: usize = 120;

/// Чем декодируется видео.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DecodeMode {
    /// D3D11VA: MFT пишет в текстуры GPU.
    Hardware,
    /// Программный MFT ОС.
    Software,
}

/// Один MFT и всё, что нужно для работы с ним; режим (GPU или нет) задан при создании.
struct MftDecoder {
    api: &'static Api,
    mft: IMFTransform,
    /// Устройство D3D11 и пул текстур; `None` — программный путь.
    gpu: Option<Gpu>,
    prefer_p010: bool,
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
// выходят — кадры копируются в `Vec<u8>`. Устройство D3D11 защищено
// `SetMultithreadProtected`, а все его интерфейсы живут в этом же объекте.
unsafe impl Send for MftDecoder {}

fn hr(what: &str, e: windows::core::Error) -> String {
    format!("{what}: {e}")
}

impl MftDecoder {
    fn new(track: &TrackInfo, gpu: Option<Gpu>) -> Result<Self, String> {
        ensure_com();
        let api = Api::get().ok_or("Media Foundation недоступна")?;
        let (subtype, params) = match track.codec {
            Codec::H264 => (MFVideoFormat_H264, annexb::parse_avcc(&track.extradata)?),
            Codec::Hevc => (MFVideoFormat_HEVC, annexb::parse_hvcc(&track.extradata)?),
            other => return Err(format!("Media Foundation: видеокодек {other:?} не поддерживается")),
        };
        let mft: IMFTransform = api.create_transform(MFT_CATEGORY_VIDEO_DECODER, MFMediaType_Video, subtype)?;
        if let Some(gpu) = &gpu {
            // SAFETY: атрибуты и сообщение MFT; менеджер живёт в `gpu`, то есть в этом же декодере.
            unsafe {
                let aware = mft.GetAttributes().ok().and_then(|a| a.GetUINT32(&MF_SA_D3D11_AWARE).ok());
                if aware != Some(1) {
                    return Err("MFT не поддерживает D3D11".to_string());
                }
                mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, gpu.manager_param())
                    .map_err(|e| hr("SET_D3D_MANAGER", e))?;
            }
        }

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
            gpu,
            prefer_p010: track.codec == Codec::Hevc && track.codec_string.contains(".2."),
            params,
            out: None,
            provides_samples: false,
            out_bytes: MIN_OUTPUT_BYTES,
            ready: VecDeque::new(),
            last_pts_us: 0,
        };
        // Выходной тип выбирается заранее, если MFT уже готов его назвать; иначе
        // он придёт с `MF_E_TRANSFORM_STREAM_CHANGE` на первом кадре.
        // На D3D-пути выходной тип нужен до первого кадра (пул текстур): без него — сбой.
        let selected = dec.select_output();
        if dec.gpu.is_some() {
            selected?;
        }
        // SAFETY: сообщения потоковой модели MFT.
        unsafe {
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).map_err(|e| hr("BEGIN_STREAMING", e))?;
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).map_err(|e| hr("START_OF_STREAM", e))?;
        }
        Ok(dec)
    }

    /// Выбрать из предложенных MFT выходных типов NV12 (для 10 бит — P010) и
    /// прочитать получившийся формат.
    fn select_output(&mut self) -> Result<(), String> {
        let wanted = if self.prefer_p010 { [MFVideoFormat_P010, MFVideoFormat_NV12] } else { [MFVideoFormat_NV12, MFVideoFormat_P010] };
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
        if !self.provides_samples
            && let Some(gpu) = &mut self.gpu
        {
            gpu.init_allocator(self.api, &t)?;
        }
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
            } else if let Some(gpu) = &self.gpu {
                Some(gpu.allocate_sample()?)
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
                    self.select_output()?;
                }
                Err(e) => return Err(hr("ProcessOutput", e)),
            }
        }
    }

    /// Скопировать образец MFT в собственный кадр (MFT переиспользует буферы).
    fn take_frame(&mut self, sample: &IMFSample) -> Result<(), String> {
        let fmt = self.out.ok_or("кадр до выбора выходного типа")?;
        if let Some(gpu) = &mut self.gpu {
            // SAFETY: чтение метки времени образца.
            let pts_us = unsafe { sample.GetSampleTime() }.map_or(self.last_pts_us, |t| t / 10);
            self.last_pts_us = pts_us;
            let frame = gpu.read_frame(sample)?;
            self.ready.push_back(Box::new(MfFrame {
                pts_us,
                layout: Layout {
                    width: fmt.width,
                    height: fmt.height,
                    stride: frame.stride,
                    uv_offset: frame.stride * frame.rows,
                    bytes_per_sample: fmt.bytes_per_sample,
                    matrix: fmt.matrix,
                    full_range: fmt.full_range,
                },
                data: frame.data,
            }));
            return Ok(());
        }
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

impl MftDecoder {
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

/// Видеодекодер бэкенда: сначала D3D11VA, при сбое — программный MFT.
pub(super) struct MfVideoDecoder {
    track: TrackInfo,
    inner: MftDecoder,
    /// Пока аппаратный декодер не выдал первый кадр: что ему отправлено после
    /// последней перемотки. Нужно, чтобы при сбое повторить это программному.
    probation: Option<Vec<(Packet, bool)>>,
}

impl MfVideoDecoder {
    pub(super) fn new(track: &TrackInfo) -> Result<Self, String> {
        if !super::software_only() {
            let hardware = Api::get()
                .ok_or_else(|| "Media Foundation недоступна".to_string())
                .and_then(Gpu::create)
                .and_then(|gpu| {
                    let adapter = gpu.adapter.describe();
                    MftDecoder::new(track, Some(gpu)).map(|d| (d, adapter))
                });
            match hardware {
                Ok((inner, adapter)) => {
                    eprintln!("[media] видео {:?}: Media Foundation, D3D11VA на {adapter}", track.codec);
                    return Ok(Self { track: track.clone(), inner, probation: Some(Vec::new()) });
                }
                Err(e) => eprintln!("[media] видео {:?}: D3D11VA недоступен ({e}), программный MFT", track.codec),
            }
        }
        let inner = MftDecoder::new(track, None)?;
        Ok(Self { track: track.clone(), inner, probation: None })
    }

    /// Чем сейчас декодируется видео.
    #[cfg(test)]
    pub(super) fn mode(&self) -> DecodeMode {
        if self.inner.gpu.is_some() { DecodeMode::Hardware } else { DecodeMode::Software }
    }

    /// Аппаратный путь не заработал: заново декодировать программным MFT всё, что
    /// отправлено с последней перемотки.
    fn fall_back(&mut self, why: &str) -> Result<(), String> {
        eprintln!("[media] видео {:?}: D3D11VA не заработал ({why}), переход на программный MFT", self.track.codec);
        let history = self.probation.take().unwrap_or_default();
        let mut software = MftDecoder::new(&self.track, None)?;
        for (packet, restart) in &history {
            software.send(packet, *restart)?;
        }
        self.inner = software;
        Ok(())
    }
}

impl VideoFrameDecoder for MfVideoDecoder {
    fn send(&mut self, packet: &Packet, restart: bool) -> Result<(), String> {
        let Some(history) = self.probation.as_mut() else { return self.inner.send(packet, restart) };
        history.push((packet.clone(), restart));
        let why = match self.inner.send(packet, restart) {
            Ok(()) if !self.inner.ready.is_empty() => {
                self.probation = None;
                return Ok(());
            }
            Ok(()) if history.len() <= PROBATION_PACKETS => return Ok(()),
            Ok(()) => format!("{} пакетов без кадра", history.len()),
            Err(e) => e,
        };
        self.fall_back(&why)
    }

    fn next_frame(&mut self) -> Option<Box<dyn DecodedFrame>> {
        self.inner.ready.pop_front()
    }

    fn drain(&mut self) -> Result<(), String> {
        let Some(history) = &self.probation else { return self.inner.drain() };
        let sent = !history.is_empty();
        let why = match self.inner.drain() {
            Ok(()) if !sent || !self.inner.ready.is_empty() => {
                self.probation = None;
                return Ok(());
            }
            Ok(()) => "пакеты отправлены, кадров нет".to_string(),
            Err(e) => e,
        };
        self.fall_back(&why)?;
        self.inner.drain()
    }

    fn flush(&mut self) {
        if let Some(history) = &mut self.probation {
            history.clear();
        }
        self.inner.flush();
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

#[cfg(test)]
mod tests {
    use lumen_media_demux::{Demuxer, TrackKind};

    use super::*;

    const H264_320: &[u8] = include_bytes!("../../tests/data/h264-aac.mp4");
    const H264_1080: &[u8] = include_bytes!("../../tests/data/h264-1080p.mp4");
    const HEVC: &[u8] = include_bytes!("../../../media-demux/tests/data/hevc.mp4");

    /// Дорожка и её пакеты в порядке контейнера.
    fn video_packets(bytes: &[u8]) -> (TrackInfo, Vec<Packet>) {
        let mut demux = Demuxer::open(bytes).unwrap();
        let track = demux.tracks().iter().find(|t| t.kind == TrackKind::Video).unwrap().clone();
        let mut packets = Vec::new();
        while let Some(p) = demux.next_packet().unwrap() {
            if p.track == track.id {
                packets.push(p);
            }
        }
        (track, packets)
    }

    /// Все кадры потока: (метка, RGBA).
    fn decode_all(dec: &mut dyn VideoFrameDecoder, packets: &[Packet]) -> Vec<(i64, Vec<u8>)> {
        let mut frames = Vec::new();
        let mut collect = |dec: &mut dyn VideoFrameDecoder| {
            while let Some(f) = dec.next_frame() {
                frames.push((f.pts_us(), f.to_rgba8().unwrap()));
            }
        };
        for (i, p) in packets.iter().enumerate() {
            dec.send(p, i == 0).unwrap();
            collect(dec);
        }
        dec.drain().unwrap();
        collect(dec);
        frames
    }

    /// Декодер, которому аппаратный путь не предлагался.
    fn software(track: &TrackInfo) -> MfVideoDecoder {
        MfVideoDecoder { track: track.clone(), inner: MftDecoder::new(track, None).unwrap(), probation: None }
    }

    fn os_has(codec: Codec) -> bool {
        let a = super::super::availability();
        match codec {
            Codec::H264 => a.h264,
            _ => a.hevc,
        }
    }

    /// Аппаратный и программный декодеры дают кадры, различающиеся не больше чем
    /// на 2 в канале. Нет GPU, адаптер в блок-листе или `LUMEN_OS_DECODERS=sw` —
    /// аппаратного пути нет, и сравнивать нечего.
    fn assert_hardware_matches_software(bytes: &[u8]) {
        let (track, packets) = video_packets(bytes);
        if !os_has(track.codec) || super::super::software_only() {
            return;
        }
        let mut hw = MfVideoDecoder::new(&track).unwrap();
        let mut sw = software(&track);
        let hw_frames = decode_all(&mut hw, &packets);
        let sw_frames = decode_all(&mut sw, &packets);
        if hw.mode() != DecodeMode::Hardware {
            eprintln!("D3D11VA недоступен на этой машине — сравнение пропущено");
            return;
        }
        assert_eq!(hw_frames.len(), packets.len(), "аппаратный путь выдал все кадры");
        assert_eq!(hw_frames.len(), sw_frames.len());
        let mut worst = 0u8;
        for ((hp, h), (sp, s)) in hw_frames.iter().zip(&sw_frames) {
            assert_eq!(hp, sp, "метки времени");
            assert_eq!(h.len(), s.len());
            worst = h.iter().zip(s).map(|(a, b)| a.abs_diff(*b)).fold(worst, u8::max);
        }
        assert!(worst <= 2, "расхождение GPU и CPU до {worst} в канале");
        eprintln!("D3D11VA: {} кадров, наибольшее расхождение с программным MFT — {worst}", hw_frames.len());
    }

    #[test]
    fn h264_320_hardware_matches_software() {
        assert_hardware_matches_software(H264_320);
    }

    #[test]
    fn h264_1080p_hardware_matches_software() {
        assert_hardware_matches_software(H264_1080);
    }

    #[test]
    fn hevc_hardware_matches_software() {
        assert_hardware_matches_software(HEVC);
    }

    /// Без аппаратного пути (`probation` пуст) декодер работает как раньше.
    #[test]
    fn software_decoder_decodes_every_frame() {
        let (track, packets) = video_packets(H264_320);
        if !os_has(track.codec) {
            return;
        }
        let mut sw = software(&track);
        let frames = decode_all(&mut sw, &packets);
        assert_eq!(frames.len(), packets.len());
        assert!(frames.iter().all(|(_, rgba)| rgba.len() == 320 * 180 * 4));
    }

    /// Сбой аппаратного пути до первого кадра: программный MFT получает те же
    /// пакеты с самого начала, и в итоге выданы все кадры без потерь.
    #[test]
    fn fallback_replays_packets_sent_before_the_failure() {
        let (track, packets) = video_packets(H264_320);
        if !os_has(track.codec) || super::super::software_only() {
            return;
        }
        let mut dec = MfVideoDecoder::new(&track).unwrap();
        if dec.mode() != DecodeMode::Hardware {
            return;
        }
        dec.send(&packets[0], true).unwrap();
        dec.send(&packets[1], false).unwrap();
        assert_eq!(dec.probation.as_ref().map(Vec::len), Some(2), "кадров ещё нет — пакеты помнятся");
        dec.fall_back("проверка отката").unwrap();
        assert_eq!(dec.mode(), DecodeMode::Software);
        assert!(dec.probation.is_none());
        let mut frames = Vec::new();
        for p in &packets[2..] {
            dec.send(p, false).unwrap();
            while let Some(f) = dec.next_frame() {
                frames.push(f.pts_us());
            }
        }
        dec.drain().unwrap();
        while let Some(f) = dec.next_frame() {
            frames.push(f.pts_us());
        }
        assert_eq!(frames.len(), packets.len());
    }

    /// Перемотка до первого кадра сбрасывает накопленные для отката пакеты.
    #[test]
    fn flush_clears_probation_history() {
        let (track, packets) = video_packets(H264_320);
        if !os_has(track.codec) || super::super::software_only() {
            return;
        }
        let mut dec = MfVideoDecoder::new(&track).unwrap();
        dec.send(&packets[0], true).unwrap();
        dec.flush();
        if let Some(h) = &dec.probation {
            assert!(h.is_empty());
        }
        let frames = decode_all(&mut dec, &packets);
        assert_eq!(frames.len(), packets.len());
    }
}
