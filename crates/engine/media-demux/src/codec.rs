//! RFC 6381-строки кодеков по их конфигурационным боксам.

use crate::bytes::Be;

/// `avc1.PPCCLL` по `avcC` (профиль, совместимость, уровень).
pub(crate) fn avc_string(fourcc: &[u8; 4], avcc: &[u8]) -> String {
    let tag = String::from_utf8_lossy(fourcc);
    match avcc {
        [_, p, c, l, ..] => format!("{tag}.{p:02x}{c:02x}{l:02x}"),
        _ => tag.into_owned(),
    }
}

/// `hvc1.1.6.L93.B0` по `hvcC` (RFC 6381 §3.3 / ISO 14496-15 Annex E).
pub(crate) fn hevc_string(fourcc: &[u8; 4], hvcc: &[u8]) -> String {
    let tag = String::from_utf8_lossy(fourcc);
    let mut r = Be::new(hvcc);
    let parsed = (|| {
        r.skip(1)?;
        let b = r.u8()?;
        let compat = r.u32()?;
        let constraints = r.bytes(6)?;
        let level = r.u8()?;
        Some((b, compat, constraints, level))
    })();
    let Some((b, compat, constraints, level)) = parsed else {
        return tag.into_owned();
    };
    let space = match b >> 6 {
        1 => "A",
        2 => "B",
        3 => "C",
        _ => "",
    };
    let tier = if b & 0x20 != 0 { 'H' } else { 'L' };
    let mut tail: Vec<String> = constraints.iter().map(|c| format!("{c:X}")).collect();
    while tail.last().is_some_and(|s| s == "0") {
        tail.pop();
    }
    let mut s = format!(
        "{tag}.{space}{}.{:X}.{tier}{level}",
        b & 0x1f,
        compat.reverse_bits()
    );
    for c in tail {
        s.push('.');
        s.push_str(&c);
    }
    s
}

/// `av01.P.LLT.DD` по полям `av1C`.
pub(crate) fn av1_string(profile: u8, level: u8, high_tier: bool, depth: u8) -> String {
    let tier = if high_tier { 'H' } else { 'M' };
    format!("av01.{profile}.{level:02}{tier}.{depth:02}")
}

/// `av01…` по самому `av1C` (4 байта заголовка + OBU конфигурации).
pub(crate) fn av1_string_from_config(av1c: &[u8]) -> String {
    match av1c {
        [_, b1, b2, ..] => {
            let depth = match (b2 >> 6 & 1, b2 >> 5 & 1) {
                (1, 1) => 12,
                (1, 0) => 10,
                _ => 8,
            };
            av1_string(b1 >> 5, b1 & 0x1f, b2 >> 7 != 0, depth)
        }
        _ => "av01".to_string(),
    }
}

/// `vp09.PP.LL.DD` по `vpcC` (FullBox: 4 байта версии/флагов + поля).
pub(crate) fn vp9_string(vpcc: &[u8]) -> String {
    match vpcc {
        [_, _, _, _, profile, level, packed, ..] => {
            format!("vp09.{profile:02}.{level:02}.{:02}", packed >> 4)
        }
        _ => "vp09".to_string(),
    }
}

/// `mp4a.40.N` по AudioObjectType.
pub(crate) fn aac_string(audio_object_type: u16) -> String {
    format!("mp4a.40.{audio_object_type}")
}

/// AudioObjectType из первых бит `AudioSpecificConfig` (5 бит, 31 → расширение).
pub(crate) fn aot_from_asc(asc: &[u8]) -> Option<u16> {
    let b = *asc.first()?;
    let aot = u16::from(b >> 3);
    if aot != 31 {
        return Some(aot);
    }
    let b2 = *asc.get(1)?;
    Some(32 + u16::from(((b & 7) << 3) | (b2 >> 5)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avc() {
        assert_eq!(avc_string(b"avc1", &[1, 0x64, 0, 0x1f, 0xff]), "avc1.64001f");
    }

    #[test]
    fn hevc_main() {
        // Main, L93, compat 0x60000000 (биты 1 и 2) → «6», constraint B0.
        let mut h = vec![1u8, 0x01];
        h.extend_from_slice(&0x6000_0000u32.to_be_bytes());
        h.extend_from_slice(&[0xB0, 0, 0, 0, 0, 0, 93]);
        assert_eq!(hevc_string(b"hev1", &h), "hev1.1.6.L93.B0");
    }

    #[test]
    fn av1() {
        assert_eq!(av1_string_from_config(&[0x81, 0x00, 0x0C, 0x00]), "av01.0.00M.08");
        assert_eq!(av1_string(0, 5, false, 8), "av01.0.05M.08");
    }

    #[test]
    fn aot() {
        assert_eq!(aot_from_asc(&[0x12, 0x10]), Some(2));
        assert_eq!(aot_from_asc(&[]), None);
    }
}
