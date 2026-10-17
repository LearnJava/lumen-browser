//! `ES_Descriptor` вокруг `AudioSpecificConfig` (ISO 14496-1 §7.2.6): так выглядит
//! «magic cookie» AAC для `AudioConverter` (AudioToolbox) — содержимое `esds` без
//! заголовка бокса. Демуксер отдаёт голый `AudioSpecificConfig`.
//!
//! Модуль не зависит от платформы: собирается и проверяется на любой ОС.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// Дескриптор: тег, длина в «расширяемом» виде из четырёх байт (как у muxer'ов
/// MP4, такой вид читают все разборщики), затем содержимое.
fn push_descriptor(out: &mut Vec<u8>, tag: u8, body: &[u8]) {
    let len = body.len() as u32;
    out.push(tag);
    out.extend_from_slice(&[
        ((len >> 21) & 0x7F) as u8 | 0x80,
        ((len >> 14) & 0x7F) as u8 | 0x80,
        ((len >> 7) & 0x7F) as u8 | 0x80,
        (len & 0x7F) as u8,
    ]);
    out.extend_from_slice(body);
}

/// `ES_Descriptor` с `DecoderConfigDescriptor` (MPEG-4 Audio, objectType 0x40) и
/// `DecoderSpecificInfo` = `audio_specific_config`.
pub(crate) fn aac_magic_cookie(audio_specific_config: &[u8]) -> Vec<u8> {
    let mut specific = Vec::new();
    push_descriptor(&mut specific, 0x05, audio_specific_config);

    // objectTypeIndication 0x40 (Audio ISO/IEC 14496-3), streamType 5 (audio) << 2 | 1,
    // bufferSizeDB (3 байта), maxBitrate, avgBitrate — декодеру они не нужны.
    let mut config = vec![0x40, 0x15, 0, 0, 0];
    config.extend_from_slice(&[0; 8]);
    config.extend_from_slice(&specific);

    // ES_ID (2 байта) и флаги потока (нет зависимостей, URL, OCR).
    let mut es = vec![0, 0, 0];
    push_descriptor(&mut es, 0x04, &config);
    // SLConfigDescriptor, predefined = 2 (MP4).
    push_descriptor(&mut es, 0x06, &[0x02]);

    let mut out = Vec::new();
    push_descriptor(&mut out, 0x03, &es);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Читает длину «расширяемого» вида.
    fn read_len(data: &[u8]) -> (usize, usize) {
        let mut len = 0usize;
        for (i, &b) in data.iter().enumerate() {
            len = (len << 7) | usize::from(b & 0x7F);
            if b & 0x80 == 0 {
                return (len, i + 1);
            }
        }
        (len, data.len())
    }

    #[test]
    fn cookie_nests_the_config_in_three_descriptors() {
        let asc = [0x12, 0x10];
        let cookie = aac_magic_cookie(&asc);
        assert_eq!(cookie[0], 0x03);
        let (es_len, n) = read_len(&cookie[1..]);
        assert_eq!(1 + n + es_len, cookie.len(), "ES_Descriptor занимает всё");
        let es = &cookie[1 + n..];
        assert_eq!(&es[..3], &[0, 0, 0], "ES_ID и флаги");
        assert_eq!(es[3], 0x04);
        let (cfg_len, n) = read_len(&es[4..]);
        let cfg = &es[4 + n..4 + n + cfg_len];
        assert_eq!(&cfg[..2], &[0x40, 0x15]);
        // 13 байт заголовка конфигурации, затем DecoderSpecificInfo.
        assert_eq!(cfg[13], 0x05);
        let (asc_len, n) = read_len(&cfg[14..]);
        assert_eq!(&cfg[14 + n..14 + n + asc_len], &asc);
        assert_eq!(14 + n + asc_len, cfg.len());
        // После DecoderConfigDescriptor — SLConfigDescriptor.
        let sl = &es[4 + 4 + cfg_len..];
        assert_eq!(sl[0], 0x06);
        assert_eq!(sl.last(), Some(&0x02));
    }

    #[test]
    fn long_config_uses_multi_byte_length() {
        let asc = vec![7u8; 300];
        let cookie = aac_magic_cookie(&asc);
        assert!(cookie.windows(asc.len()).any(|w| w == asc.as_slice()));
        let (es_len, n) = read_len(&cookie[1..]);
        assert_eq!(1 + n + es_len, cookie.len());
    }
}
