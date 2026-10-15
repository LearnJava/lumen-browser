//! H.264 / HEVC: конфигурация `avcC` / `hvcC` и пакеты MP4 (с префиксами длины) →
//! Annex B (стартовые коды `00 00 00 01`), как ждут декодеры ОС (Media Foundation).
//!
//! Модуль не зависит от платформы: тот же разбор понадобится VideoToolbox и VA-API,
//! а тестируется он на любой ОС.

#![cfg_attr(not(windows), allow(dead_code))]

const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// Параметры потока из `avcC` / `hvcC`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParameterSets {
    /// Размер префикса длины NAL-единицы в пакетах (1–4 байта).
    pub nal_length_size: usize,
    /// VPS/SPS/PPS в Annex B — кладутся перед ключевым кадром.
    pub annexb: Vec<u8>,
}

/// Чтение из `data` с проверкой границ.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<usize> {
        let hi = usize::from(self.u8()?);
        Some((hi << 8) | usize::from(self.u8()?))
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
}

fn push_nal(out: &mut Vec<u8>, nal: &[u8]) {
    out.extend_from_slice(&START_CODE);
    out.extend_from_slice(nal);
}

/// `AVCDecoderConfigurationRecord` (ISO 14496-15 §5.3.3.1).
///
/// # Errors
/// Запись короче заголовка, список SPS/PPS обрывается или пуст.
pub(crate) fn parse_avcc(extradata: &[u8]) -> Result<ParameterSets, String> {
    let bad = || "avcC обрезан".to_string();
    let mut r = Reader { data: extradata, pos: 0 };
    r.take(4).ok_or_else(bad)?; // version, profile, compat, level
    let nal_length_size = usize::from(r.u8().ok_or_else(bad)? & 3) + 1;
    let mut annexb = Vec::new();
    let sps_count = r.u8().ok_or_else(bad)? & 0x1F;
    for _ in 0..sps_count {
        let n = r.u16().ok_or_else(bad)?;
        push_nal(&mut annexb, r.take(n).ok_or_else(bad)?);
    }
    let pps_count = r.u8().ok_or_else(bad)?;
    for _ in 0..pps_count {
        let n = r.u16().ok_or_else(bad)?;
        push_nal(&mut annexb, r.take(n).ok_or_else(bad)?);
    }
    if annexb.is_empty() {
        return Err("avcC без SPS/PPS".to_string());
    }
    Ok(ParameterSets { nal_length_size, annexb })
}

/// `HEVCDecoderConfigurationRecord` (ISO 14496-15 §8.3.3.1).
///
/// # Errors
/// Запись короче заголовка, массивы NAL-единиц обрываются или пусты.
pub(crate) fn parse_hvcc(extradata: &[u8]) -> Result<ParameterSets, String> {
    let bad = || "hvcC обрезан".to_string();
    let mut r = Reader { data: extradata, pos: 0 };
    r.take(21).ok_or_else(bad)?;
    let nal_length_size = usize::from(r.u8().ok_or_else(bad)? & 3) + 1;
    let arrays = r.u8().ok_or_else(bad)?;
    let mut annexb = Vec::new();
    for _ in 0..arrays {
        r.u8().ok_or_else(bad)?; // array_completeness + NAL_unit_type
        let count = r.u16().ok_or_else(bad)?;
        for _ in 0..count {
            let n = r.u16().ok_or_else(bad)?;
            push_nal(&mut annexb, r.take(n).ok_or_else(bad)?);
        }
    }
    if annexb.is_empty() {
        return Err("hvcC без VPS/SPS/PPS".to_string());
    }
    Ok(ParameterSets { nal_length_size, annexb })
}

/// Пакет с префиксами длины → Annex B. `prefix` (параметры потока) ставится
/// перед данными: нужен на ключевых кадрах и после перемотки.
///
/// # Errors
/// Длина NAL-единицы выходит за пакет.
pub(crate) fn to_annexb(packet: &[u8], nal_length_size: usize, prefix: &[u8]) -> Result<Vec<u8>, String> {
    if !(1..=4).contains(&nal_length_size) {
        return Err(format!("недопустимый размер префикса длины: {nal_length_size}"));
    }
    let mut out = Vec::with_capacity(prefix.len() + packet.len() + 16);
    out.extend_from_slice(prefix);
    let mut r = Reader { data: packet, pos: 0 };
    while r.pos < packet.len() {
        let mut len = 0usize;
        for _ in 0..nal_length_size {
            len = (len << 8) | usize::from(r.u8().ok_or("пакет обрезан внутри длины NAL")?);
        }
        let nal = r.take(len).ok_or("длина NAL выходит за пакет")?;
        push_nal(&mut out, nal);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avcc_yields_annexb_parameter_sets() {
        let avcc = [1, 0x64, 0, 0x1F, 0xFF, 0xE1, 0, 2, 0x67, 0x64, 1, 0, 1, 0x68];
        let ps = parse_avcc(&avcc).unwrap();
        assert_eq!(ps.nal_length_size, 4);
        assert_eq!(ps.annexb, [0, 0, 0, 1, 0x67, 0x64, 0, 0, 0, 1, 0x68]);
    }

    #[test]
    fn avcc_truncated_is_error() {
        assert!(parse_avcc(&[1, 0x64, 0, 0x1F, 0xFF, 0xE1, 0, 9, 0x67]).is_err());
        assert!(parse_avcc(&[1, 0x64]).is_err());
        assert!(parse_avcc(&[1, 0x64, 0, 0x1F, 0xFF, 0xE0, 0]).is_err());
    }

    #[test]
    fn hvcc_collects_all_arrays() {
        let mut hvcc = vec![1u8; 21];
        hvcc.push(0xFF); // lengthSizeMinusOne = 3
        hvcc.push(2); // два массива
        hvcc.extend_from_slice(&[0xA0, 0, 1, 0, 2, 0x40, 1]); // VPS
        hvcc.extend_from_slice(&[0xA1, 0, 1, 0, 2, 0x42, 1]); // SPS
        let ps = parse_hvcc(&hvcc).unwrap();
        assert_eq!(ps.nal_length_size, 4);
        assert_eq!(ps.annexb, [0, 0, 0, 1, 0x40, 1, 0, 0, 0, 1, 0x42, 1]);
        assert!(parse_hvcc(&hvcc[..30]).is_err());
    }

    #[test]
    fn packet_length_prefixes_become_start_codes() {
        let packet = [0, 0, 0, 2, 0x65, 0xAA, 0, 0, 0, 1, 0x41];
        let out = to_annexb(&packet, 4, &[0, 0, 0, 1, 0x67]).unwrap();
        assert_eq!(out, [0, 0, 0, 1, 0x67, 0, 0, 0, 1, 0x65, 0xAA, 0, 0, 0, 1, 0x41]);
        let two = to_annexb(&[0, 1, 0x41], 2, &[]).unwrap();
        assert_eq!(two, [0, 0, 0, 1, 0x41]);
    }

    #[test]
    fn packet_with_overlong_nal_is_error() {
        assert!(to_annexb(&[0, 0, 0, 9, 1, 2], 4, &[]).is_err());
        assert!(to_annexb(&[0, 0], 4, &[]).is_err());
        assert!(to_annexb(&[1], 0, &[]).is_err());
    }
}
