//! AAC через MFT-декодер ОС: сырые кадры AAC из MP4 → `IMFTransform` → PCM i16.

use std::mem::ManuallyDrop;

use lumen_media_demux::{Packet, TrackInfo};
use windows::Win32::Media::MediaFoundation::{
    IMFTransform, MFAudioFormat_AAC, MFAudioFormat_PCM, MFMediaType_Audio, MFSampleExtension_Discontinuity,
    MFT_CATEGORY_AUDIO_DECODER, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, MF_MT_AAC_PAYLOAD_TYPE, MF_MT_AUDIO_BITS_PER_SAMPLE,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_MT_USER_DATA,
};

use super::{ensure_com, Api};
use crate::registry::AudioPcmDecoder;

/// Профиль/уровень AAC LC (`AudioProfileLevelIndication` = 0x29, как у MP4-источника ОС).
const PROFILE_LEVEL_LC: u32 = 0x29;
const MIN_OUTPUT_BYTES: u32 = 32 * 1024;

pub(super) struct MfAacDecoder {
    api: &'static Api,
    mft: IMFTransform,
    provides_samples: bool,
    out_bytes: u32,
    restart: bool,
}

// SAFETY: как у `MfVideoDecoder` — MFT free-threaded, доступ только через `&mut self`.
unsafe impl Send for MfAacDecoder {}

fn hr(what: &str, e: windows::core::Error) -> String {
    format!("{what}: {e}")
}

impl MfAacDecoder {
    pub(super) fn new(track: &TrackInfo) -> Result<Self, String> {
        ensure_com();
        let api = Api::get().ok_or("Media Foundation недоступна")?;
        let mft: IMFTransform = api.create_transform(MFT_CATEGORY_AUDIO_DECODER, MFMediaType_Audio, MFAudioFormat_AAC)?;

        // Хвост `HEAACWAVEINFO` (payload = 0: сырой AAC, без ADTS) + AudioSpecificConfig.
        let mut user_data = Vec::with_capacity(12 + track.extradata.len());
        user_data.extend_from_slice(&0u16.to_le_bytes());
        user_data.extend_from_slice(&(PROFILE_LEVEL_LC as u16).to_le_bytes());
        user_data.extend_from_slice(&[0; 8]);
        user_data.extend_from_slice(&track.extradata);

        let input = api.new_media_type()?;
        // SAFETY: тип создан выше; ключи — константы Media Foundation.
        unsafe {
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(|e| hr("MAJOR_TYPE", e))?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).map_err(|e| hr("SUBTYPE", e))?;
            input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(track.channels)).map_err(|e| hr("CHANNELS", e))?;
            input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, track.sample_rate).map_err(|e| hr("RATE", e))?;
            input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(|e| hr("BITS", e))?;
            input.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0).map_err(|e| hr("PAYLOAD_TYPE", e))?;
            input.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, PROFILE_LEVEL_LC).map_err(|e| hr("LEVEL", e))?;
            input.SetBlob(&MF_MT_USER_DATA, &user_data).map_err(|e| hr("USER_DATA", e))?;
            mft.SetInputType(0, &input, 0).map_err(|e| hr("SetInputType", e))?;
        }
        let mut dec = Self { api, mft, provides_samples: false, out_bytes: MIN_OUTPUT_BYTES, restart: true };
        dec.select_output()?;
        // SAFETY: сообщения потоковой модели MFT.
        unsafe {
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).map_err(|e| hr("BEGIN_STREAMING", e))?;
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).map_err(|e| hr("START_OF_STREAM", e))?;
        }
        Ok(dec)
    }

    /// Выбрать выходной тип PCM 16 бит и прочитать параметры выходного потока.
    fn select_output(&mut self) -> Result<(), String> {
        for i in 0.. {
            // SAFETY: перебор типов, пока MFT не ответит `MF_E_NO_MORE_TYPES`.
            let Ok(t) = (unsafe { self.mft.GetOutputAvailableType(0, i) }) else {
                return Err("декодер AAC не предлагает PCM 16 бит".to_string());
            };
            // SAFETY: чтение атрибутов типа.
            let (sub, bits) = unsafe { (t.GetGUID(&MF_MT_SUBTYPE), t.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE)) };
            if sub == Ok(MFAudioFormat_PCM) && bits == Ok(16) {
                // SAFETY: тип получен от этого же MFT.
                unsafe { self.mft.SetOutputType(0, &t, 0) }.map_err(|e| hr("SetOutputType", e))?;
                break;
            }
        }
        // SAFETY: запрос параметров выходного потока.
        let info = unsafe { self.mft.GetOutputStreamInfo(0) }.map_err(|e| hr("GetOutputStreamInfo", e))?;
        self.provides_samples = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
        self.out_bytes = info.cbSize.max(MIN_OUTPUT_BYTES);
        Ok(())
    }

    fn pull_outputs(&mut self, pcm: &mut Vec<i16>) -> Result<(), String> {
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
            // SAFETY: после вызова поля принадлежат нам.
            let sample = unsafe {
                let sample = ManuallyDrop::take(&mut data[0].pSample);
                drop(ManuallyDrop::take(&mut data[0].pEvents));
                sample
            };
            match result {
                Ok(()) => {
                    let Some(sample) = sample else { continue };
                    // SAFETY: образец от MFT; Lock/Unlock парные.
                    unsafe {
                        let buffer = sample.ConvertToContiguousBuffer().map_err(|e| hr("ConvertToContiguousBuffer", e))?;
                        let mut ptr = std::ptr::null_mut();
                        let mut len = 0u32;
                        buffer.Lock(&mut ptr, None, Some(&mut len)).map_err(|e| hr("Lock", e))?;
                        let bytes = std::slice::from_raw_parts(ptr, len as usize);
                        pcm.extend(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])));
                        buffer.Unlock().map_err(|e| hr("Unlock", e))?;
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.select_output()?,
                Err(e) => return Err(hr("ProcessOutput", e)),
            }
        }
    }
}

impl AudioPcmDecoder for MfAacDecoder {
    fn decode(&mut self, packet: &Packet) -> Result<Vec<i16>, String> {
        let len = u32::try_from(packet.data.len()).map_err(|_| "пакет больше 4 ГиБ".to_string())?;
        let buffer = self.api.new_buffer(len)?;
        // SAFETY: буфер создан на `len` байт; Lock/Unlock парные.
        unsafe {
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None).map_err(|e| hr("Lock", e))?;
            std::ptr::copy_nonoverlapping(packet.data.as_ptr(), ptr, packet.data.len());
            buffer.Unlock().map_err(|e| hr("Unlock", e))?;
            buffer.SetCurrentLength(len).map_err(|e| hr("SetCurrentLength", e))?;
        }
        let sample = self.api.new_sample()?;
        // SAFETY: образец создан выше; время — в единицах по 100 нс.
        unsafe {
            sample.AddBuffer(&buffer).map_err(|e| hr("AddBuffer", e))?;
            sample.SetSampleTime(packet.pts_us.saturating_mul(10)).map_err(|e| hr("SetSampleTime", e))?;
            sample.SetSampleDuration(packet.duration_us.max(0).saturating_mul(10)).map_err(|e| hr("SetSampleDuration", e))?;
            if std::mem::take(&mut self.restart) {
                let _ = sample.SetUINT32(&MFSampleExtension_Discontinuity, 1);
            }
        }
        let mut pcm = Vec::new();
        // SAFETY: образец готов; при `MF_E_NOTACCEPTING` MFT сначала опустошаем.
        let mut result = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        if result.as_ref().is_err_and(|e| e.code() == MF_E_NOTACCEPTING) {
            self.pull_outputs(&mut pcm)?;
            // SAFETY: повтор после опустошения.
            result = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        }
        result.map_err(|e| hr("ProcessInput", e))?;
        self.pull_outputs(&mut pcm)?;
        Ok(pcm)
    }

    fn reset(&mut self) {
        // SAFETY: перемотка — сбросить перекрытие окон предыдущего пакета.
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }
        self.restart = true;
    }
}
