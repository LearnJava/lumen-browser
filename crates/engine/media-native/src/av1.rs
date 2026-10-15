//! AV1: встроенный бэкенд на `rav1d` ([`lumen_av1`]), одинаковый на всех платформах.

use lumen_av1::{Config, Decoder, Picture};
use lumen_media_demux::{Codec, Packet, TrackInfo};

use crate::registry::{DecodedFrame, VideoBackend, VideoFrameDecoder};

/// Бэкенд AV1 (профили 0–2, 8/10/12 бит — всё, что открывает `rav1d`).
pub(crate) struct Av1Backend;

impl VideoBackend for Av1Backend {
    fn name(&self) -> &'static str {
        "rav1d"
    }

    fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        codec == Codec::Av1 && valid_av01(codec_string)
    }

    fn create(&self, track: &TrackInfo) -> Result<Box<dyn VideoFrameDecoder>, String> {
        let decoder = Decoder::new(&Config::video()).map_err(|e| format!("AV1: {e}"))?;
        Ok(Box::new(Av1FrameDecoder { decoder, config_obus: config_obus(&track.extradata) }))
    }
}

/// Строка `av01.P.LLT.DD[...]` (AV1-ISOBMFF §5): профиль 0–2, уровень 00–31,
/// ярус `M`/`H`, глубина 08/10/12. Короткая запись `av01` без параметров, как
/// у `TrackInfo::codec_string` дорожек, где `av1C` не прочитан, тоже годится.
fn valid_av01(s: &str) -> bool {
    if s == "av01" {
        return true;
    }
    let mut it = s.split('.');
    if it.next() != Some("av01") {
        return false;
    }
    let profile = it.next().and_then(|p| p.parse::<u8>().ok());
    let level_tier = it.next().unwrap_or("");
    let depth = it.next().and_then(|d| d.parse::<u8>().ok());
    let level_ok = level_tier.len() == 3
        && level_tier[..2].parse::<u8>().is_ok_and(|l| l <= 31)
        && matches!(level_tier.as_bytes()[2], b'M' | b'H' | b'm' | b'h');
    matches!(profile, Some(0..=2)) && level_ok && matches!(depth, Some(8 | 10 | 12))
}

/// `configOBUs` из `av1C` (4 байта заголовка, дальше OBU последовательности).
fn config_obus(extradata: &[u8]) -> Vec<u8> {
    if extradata.len() > 4 && extradata[0] & 0x80 != 0 {
        extradata[4..].to_vec()
    } else {
        Vec::new()
    }
}

struct Av1FrameDecoder {
    decoder: Decoder,
    /// OBU последовательности из `av1C`: кладутся перед первым пакетом после
    /// старта и перемотки, на случай если ключевой кадр их не повторяет.
    config_obus: Vec<u8>,
}

impl VideoFrameDecoder for Av1FrameDecoder {
    fn send(&mut self, packet: &Packet, restart: bool) -> Result<(), String> {
        let result = if restart && !self.config_obus.is_empty() {
            let mut data = Vec::with_capacity(self.config_obus.len() + packet.data.len());
            data.extend_from_slice(&self.config_obus);
            data.extend_from_slice(&packet.data);
            self.decoder.send(&data, packet.pts_us)
        } else {
            self.decoder.send(&packet.data, packet.pts_us)
        };
        result.map_err(|e| format!("AV1: {e}"))
    }

    fn next_frame(&mut self) -> Option<Box<dyn DecodedFrame>> {
        self.decoder.next_picture().map(|p| Box::new(Av1Frame(p)) as Box<dyn DecodedFrame>)
    }

    fn drain(&mut self) -> Result<(), String> {
        self.decoder.drain().map_err(|e| format!("AV1: {e}"))
    }

    fn flush(&mut self) {
        self.decoder.flush();
    }
}

struct Av1Frame(Picture);

impl DecodedFrame for Av1Frame {
    fn pts_us(&self) -> i64 {
        self.0.pts
    }

    fn width(&self) -> u32 {
        self.0.width
    }

    fn height(&self) -> u32 {
        self.0.height
    }

    fn to_rgba8(&self) -> Result<Vec<u8>, String> {
        self.0.to_rgba8().map_err(|e| format!("AV1: {e}"))
    }
}
