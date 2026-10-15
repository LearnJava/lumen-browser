//! Vorbis: встроенный бэкенд на `lewton` (чистый Rust).

use lewton::audio::{PreviousWindowRight, read_audio_packet};
use lewton::header::{IdentHeader, SetupHeader, read_header_ident, read_header_setup};
use lumen_media_demux::{Codec, Packet, TrackInfo};

use crate::registry::{AudioBackend, AudioPcmDecoder};

/// Бэкенд Vorbis.
pub(crate) struct VorbisBackend;

impl AudioBackend for VorbisBackend {
    fn name(&self) -> &'static str {
        "lewton"
    }

    fn supports(&self, codec: Codec, _codec_string: &str) -> bool {
        codec == Codec::Vorbis
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn AudioPcmDecoder>, String> {
        let [ident, _comment, setup] = split_xiph(&track.extradata)
            .ok_or_else(|| "Vorbis: заголовки дорожки повреждены".to_string())?;
        let ident = read_header_ident(&ident).map_err(|e| format!("Vorbis: заголовок: {e:?}"))?;
        let setup = read_header_setup(&setup, ident.audio_channels, (ident.blocksize_0, ident.blocksize_1))
            .map_err(|e| format!("Vorbis: таблицы декодера: {e:?}"))?;
        Ok(Box::new(VorbisDecoder { ident, setup, window: PreviousWindowRight::new() }))
    }
}

/// Три заголовка Vorbis в упаковке Xiph: `2`, длины первых двух (по 255 и
/// остатку), затем данные трёх заголовков подряд.
fn split_xiph(data: &[u8]) -> Option<[Vec<u8>; 3]> {
    if *data.first()? != 2 {
        return None;
    }
    let mut pos = 1;
    let mut sizes = [0usize; 2];
    for size in &mut sizes {
        loop {
            let b = *data.get(pos)?;
            pos += 1;
            *size += usize::from(b);
            if b != 255 {
                break;
            }
        }
    }
    let first = data.get(pos..pos.checked_add(sizes[0])?)?;
    pos += sizes[0];
    let second = data.get(pos..pos.checked_add(sizes[1])?)?;
    pos += sizes[1];
    let third = data.get(pos..)?;
    Some([first.to_vec(), second.to_vec(), third.to_vec()])
}

struct VorbisDecoder {
    ident: IdentHeader,
    setup: SetupHeader,
    window: PreviousWindowRight,
}

impl AudioPcmDecoder for VorbisDecoder {
    fn decode(&mut self, packet: &Packet) -> Result<Vec<i16>, String> {
        let planar = read_audio_packet(&self.ident, &self.setup, &packet.data, &mut self.window)
            .map_err(|e| format!("Vorbis: {e:?}"))?;
        let channels = planar.len();
        let frames = planar.first().map_or(0, Vec::len);
        let mut out = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            for channel in &planar {
                out.push(channel.get(i).copied().unwrap_or(0));
            }
        }
        Ok(out)
    }

    fn reset(&mut self) {
        self.window = PreviousWindowRight::new();
    }
}
