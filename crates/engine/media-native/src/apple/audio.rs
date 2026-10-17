//! AAC через `AudioConverter` AudioToolbox: сырые кадры AAC из MP4 → PCM i16.

use std::ffi::c_void;

use lumen_media_demux::{Packet, TrackInfo};

use super::check;
use super::ffi::{self, AudioBuffer, AudioBufferList1, AudioConverterRef, AudioStreamBasicDescription, AudioStreamPacketDescription};
use crate::esds;
use crate::registry::AudioPcmDecoder;

/// Кадров PCM, под которые выделен выходной буфер: больше, чем даёт один пакет AAC
/// (1024; 2048 у HE-AAC с расширением полосы).
const OUTPUT_FRAMES: u32 = 4096;
/// Собственный код «входные данные кончились»: конвертер возвращает то, что успел
/// выдать, и этот же код (не ошибка).
const NO_MORE_DATA: i32 = 1;

/// Форматы: сжатый AAC и интерливленный PCM i16 той же частоты и числа каналов.
fn formats(sample_rate: u32, channels: u32) -> (AudioStreamBasicDescription, AudioStreamBasicDescription) {
    let source = AudioStreamBasicDescription {
        mSampleRate: f64::from(sample_rate),
        mFormatID: ffi::kAudioFormatMPEG4AAC,
        mFramesPerPacket: 1024,
        mChannelsPerFrame: channels,
        ..AudioStreamBasicDescription::default()
    };
    let destination = AudioStreamBasicDescription {
        mSampleRate: f64::from(sample_rate),
        mFormatID: ffi::kAudioFormatLinearPCM,
        mFormatFlags: ffi::kAudioFormatFlagsPackedS16,
        mBytesPerPacket: 2 * channels,
        mFramesPerPacket: 1,
        mBytesPerFrame: 2 * channels,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 16,
        mReserved: 0,
    };
    (source, destination)
}

/// Умеет ли ОС создать конвертер AAC → PCM (проверка наличия).
pub(super) fn converter_available() -> bool {
    let (source, destination) = formats(44_100, 2);
    let mut converter: AudioConverterRef = std::ptr::null_mut();
    // SAFETY: описания форматов живут до конца вызова; `converter` — выходной параметр.
    let status = unsafe { ffi::AudioConverterNew(&raw const source, &raw const destination, &raw mut converter) };
    if status != 0 || converter.is_null() {
        return false;
    }
    // SAFETY: конвертер создан выше.
    unsafe { ffi::AudioConverterDispose(converter) };
    true
}

/// Пакет, который отдаёт обратный вызов конвертера (один раз).
struct Input {
    data: *mut c_void,
    len: u32,
    channels: u32,
    consumed: bool,
    description: AudioStreamPacketDescription,
}

unsafe extern "C" fn supply_packet(
    _converter: AudioConverterRef,
    io_packets: *mut u32,
    io_data: *mut AudioBufferList1,
    out_description: *mut *mut AudioStreamPacketDescription,
    user_data: *mut c_void,
) -> i32 {
    // SAFETY: `user_data` — `Input` из `decode`, живой на время `AudioConverterFillComplexBuffer`;
    // остальные указатели даёт конвертер.
    unsafe {
        let input = &mut *user_data.cast::<Input>();
        if input.consumed {
            *io_packets = 0;
            return NO_MORE_DATA;
        }
        input.consumed = true;
        *io_packets = 1;
        (*io_data).mNumberBuffers = 1;
        (*io_data).mBuffers[0] =
            AudioBuffer { mNumberChannels: input.channels, mDataByteSize: input.len, mData: input.data };
        if !out_description.is_null() {
            input.description =
                AudioStreamPacketDescription { mStartOffset: 0, mVariableFramesInPacket: 0, mDataByteSize: input.len };
            *out_description = &raw mut input.description;
        }
    }
    0
}

/// Декодер одной аудиодорожки AAC.
pub(super) struct AacDecoder {
    converter: AudioConverterRef,
    channels: u32,
}

// SAFETY: конвертер используется только через `&mut self`, ссылок на него наружу нет.
unsafe impl Send for AacDecoder {}

impl AacDecoder {
    pub(super) fn new(track: &TrackInfo) -> Result<Self, String> {
        let channels = u32::from(track.channels);
        if channels == 0 {
            return Err("AudioToolbox: у дорожки нет каналов".to_string());
        }
        let (source, destination) = formats(track.sample_rate, channels);
        let mut converter: AudioConverterRef = std::ptr::null_mut();
        // SAFETY: описания форматов живут до конца вызова; `converter` — выходной параметр.
        let status =
            unsafe { ffi::AudioConverterNew(&raw const source, &raw const destination, &raw mut converter) };
        check("AudioConverterNew", status)?;
        if converter.is_null() {
            return Err("AudioConverter не создан".to_string());
        }
        // С этого момента `Drop` освободит конвертер при любом выходе.
        let decoder = Self { converter, channels };
        if !track.extradata.is_empty() {
            let cookie = esds::aac_magic_cookie(&track.extradata);
            let size = u32::try_from(cookie.len()).map_err(|_| "AudioSpecificConfig слишком велик")?;
            // SAFETY: `cookie` живёт до конца вызова; конвертер копирует его.
            let status = unsafe {
                ffi::AudioConverterSetProperty(
                    decoder.converter,
                    ffi::kAudioConverterDecompressionMagicCookie,
                    size,
                    cookie.as_ptr().cast::<c_void>(),
                )
            };
            check("AudioConverterSetProperty(cookie)", status)?;
        }
        Ok(decoder)
    }
}

impl Drop for AacDecoder {
    fn drop(&mut self) {
        // SAFETY: конвертер создан `AudioConverterNew` и принадлежит нам.
        unsafe { ffi::AudioConverterDispose(self.converter) };
    }
}

impl AudioPcmDecoder for AacDecoder {
    fn decode(&mut self, packet: &Packet) -> Result<Vec<i16>, String> {
        if packet.data.is_empty() {
            return Ok(Vec::new());
        }
        let len = u32::try_from(packet.data.len()).map_err(|_| "пакет больше 4 ГиБ".to_string())?;
        let mut input = Input {
            data: packet.data.as_ptr().cast_mut().cast::<c_void>(),
            len,
            channels: self.channels,
            consumed: false,
            description: AudioStreamPacketDescription { mStartOffset: 0, mVariableFramesInPacket: 0, mDataByteSize: len },
        };
        let mut pcm = vec![0i16; OUTPUT_FRAMES as usize * self.channels as usize];
        let mut list = AudioBufferList1 {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: self.channels,
                mDataByteSize: u32::try_from(pcm.len() * 2).map_err(|_| "выходной буфер слишком велик")?,
                mData: pcm.as_mut_ptr().cast::<c_void>(),
            }],
        };
        let mut frames = OUTPUT_FRAMES;
        // SAFETY: `input` и `list` живут до конца вызова; конвертер читает пакет только
        // внутри обратного вызова (данные не изменяются) и пишет не больше `frames`
        // кадров в `pcm`.
        let status = unsafe {
            ffi::AudioConverterFillComplexBuffer(
                self.converter,
                supply_packet,
                (&raw mut input).cast::<c_void>(),
                &raw mut frames,
                &raw mut list,
                std::ptr::null_mut(),
            )
        };
        if status != 0 && status != NO_MORE_DATA {
            return Err(format!("AudioConverterFillComplexBuffer: OSStatus {status}"));
        }
        pcm.truncate(frames.min(OUTPUT_FRAMES) as usize * self.channels as usize);
        Ok(pcm)
    }

    fn reset(&mut self) {
        // SAFETY: конвертер жив; сбрасывает перекрытие окон предыдущего пакета.
        unsafe { ffi::AudioConverterReset(self.converter) };
    }
}
