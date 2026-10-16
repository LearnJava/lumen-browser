//! Нарезка потока байтов `appendBuffer` на init- и media-сегменты (MSE Byte Stream
//! Format Registry: ISO BMFF и WebM).
//!
//! `appendBuffer` может принести часть сегмента или несколько подряд: разборщик
//! копит байты и отдаёт сегмент, только когда он пришёл целиком.

use crate::Format;

/// Самый большой бокс/элемент, который имеет смысл ждать: больше — поток битый
/// (иначе мусор в заголовке заставил бы копить гигабайты).
const MAX_UNIT: u64 = 1 << 30;

const ID_EBML: u32 = 0x1A45_DFA3;
const ID_SEGMENT: u32 = 0x1853_8067;
const ID_INFO: u32 = 0x1549_A966;
const ID_TRACKS: u32 = 0x1654_AE6B;
const ID_CLUSTER: u32 = 0x1F43_B675;
/// Элементы первого уровня Segment: после них кончается Cluster неизвестной длины.
const LEVEL1: [u32; 8] = [
    ID_CLUSTER,
    ID_INFO,
    ID_TRACKS,
    0x1C53_BB6B, // Cues
    0x114D_9B74, // SeekHead
    0x1254_C367, // Tags
    0x1941_A469, // Attachments
    0x1043_A770, // Chapters
];

/// `Info` с одним `TimestampScale` = 1 мс — на случай потока без `Info`.
const DEFAULT_INFO: [u8; 12] = [0x15, 0x49, 0xA9, 0x66, 0x87, 0x2A, 0xD7, 0xB1, 0x83, 0x0F, 0x42, 0x40];

/// Пустой `Cluster` (только `Timecode`): читатель Matroska не открывает файл без кластера,
/// а дорожки init-сегмента нужны до первого кадра.
const EMPTY_CLUSTER: [u8; 8] = [0x1F, 0x43, 0xB6, 0x75, 0x83, 0xE7, 0x81, 0x00];

/// Initialization segment: всё, что демуксеру нужно до первого кадра.
#[derive(Clone, Debug)]
pub(crate) struct Init {
    /// MP4: `ftyp`+`moov`. WebM: EBML-заголовок, затем (с `body_start`) `Info`+`Tracks`.
    pub bytes: Vec<u8>,
    /// Начало части, которая в WebM лежит внутри `Segment` (для MP4 — 0).
    pub body_start: usize,
}

/// Кусок потока, разобранный до границы сегмента.
pub(crate) enum Unit {
    /// Initialization segment.
    Init(Init),
    /// Media segment: MP4 — `moof`…`mdat`, WebM — один `Cluster`.
    Media(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Ждём EBML-заголовок.
    Ebml,
    /// Ждём заголовок `Segment`.
    Segment,
    /// Элементы внутри `Segment`.
    Children,
}

/// Накопитель разбора: состояние между вызовами `appendBuffer`.
pub(crate) struct Splitter {
    format: Format,
    /// MP4: накопленные `ftyp`/`moov`. WebM: EBML-заголовок.
    head: Vec<u8>,
    /// WebM: `Info` текущего init-сегмента.
    info: Option<Vec<u8>>,
    stage: Stage,
}

impl Splitter {
    pub(crate) fn new(format: Format) -> Self {
        Self { format, head: Vec::new(), info: None, stage: Stage::Ebml }
    }

    /// Снять с начала `buf` следующий целиком пришедший сегмент.
    ///
    /// # Errors
    /// Строка с причиной — байты не похожи на нужный контейнер.
    pub(crate) fn next(&mut self, buf: &mut Vec<u8>) -> Result<Option<Unit>, String> {
        match self.format {
            Format::Mp4 => self.next_mp4(buf),
            Format::WebM => self.next_webm(buf),
        }
    }

    // ── ISO BMFF ──────────────────────────────────────────────────────────

    fn next_mp4(&mut self, buf: &mut Vec<u8>) -> Result<Option<Unit>, String> {
        loop {
            let Some((size, kind)) = box_header(buf)? else { return Ok(None) };
            if buf.len() < size {
                return Ok(None);
            }
            match &kind {
                b"ftyp" => {
                    self.head.clear();
                    self.head.extend_from_slice(&buf[..size]);
                    buf.drain(..size);
                }
                b"moov" => {
                    let mut bytes = std::mem::take(&mut self.head);
                    bytes.extend_from_slice(&buf[..size]);
                    buf.drain(..size);
                    return Ok(Some(Unit::Init(Init { bytes, body_start: 0 })));
                }
                b"moof" => {
                    // Дальше до `mdat` могут стоять `free`/`sidx` — они входят в сегмент.
                    let mut end = size;
                    loop {
                        let Some((next, next_kind)) = box_header(&buf[end..])? else { return Ok(None) };
                        if buf.len() < end + next {
                            return Ok(None);
                        }
                        end += next;
                        if &next_kind == b"mdat" {
                            break;
                        }
                    }
                    let media: Vec<u8> = buf.drain(..end).collect();
                    return Ok(Some(Unit::Media(media)));
                }
                _ => {
                    buf.drain(..size);
                }
            }
        }
    }

    // ── WebM ──────────────────────────────────────────────────────────────

    fn next_webm(&mut self, buf: &mut Vec<u8>) -> Result<Option<Unit>, String> {
        loop {
            let Some((id, id_len)) = read_id(buf)? else { return Ok(None) };
            let Some((size, size_len)) = read_size(&buf[id_len..])? else { return Ok(None) };
            let head_len = id_len + size_len;
            match (self.stage, id) {
                (_, ID_EBML) => {
                    let total = known_total(head_len, size)?;
                    if buf.len() < total {
                        return Ok(None);
                    }
                    self.head = buf[..total].to_vec();
                    self.info = None;
                    self.stage = Stage::Segment;
                    buf.drain(..total);
                }
                (Stage::Ebml, _) => return Err("поток WebM не начинается с EBML-заголовка".into()),
                (Stage::Segment, ID_SEGMENT) => {
                    buf.drain(..head_len);
                    self.stage = Stage::Children;
                }
                (Stage::Segment, _) => return Err("после EBML-заголовка нет элемента Segment".into()),
                (Stage::Children, ID_CLUSTER) => {
                    let total = match size {
                        Some(n) => known_total(head_len, Some(n))?,
                        None => match unknown_cluster_len(&buf[head_len..])? {
                            Some(n) => head_len + n,
                            None => return Ok(None),
                        },
                    };
                    if buf.len() < total {
                        return Ok(None);
                    }
                    let media: Vec<u8> = buf.drain(..total).collect();
                    return Ok(Some(Unit::Media(media)));
                }
                (Stage::Children, _) => {
                    let total = known_total(head_len, size)?;
                    if buf.len() < total {
                        return Ok(None);
                    }
                    match id {
                        ID_INFO => self.info = Some(buf[..total].to_vec()),
                        ID_TRACKS => {
                            let mut bytes = self.head.clone();
                            let body_start = bytes.len();
                            bytes.extend_from_slice(self.info.as_deref().unwrap_or(&DEFAULT_INFO));
                            bytes.extend_from_slice(&buf[..total]);
                            buf.drain(..total);
                            return Ok(Some(Unit::Init(Init { bytes, body_start })));
                        }
                        _ => {}
                    }
                    buf.drain(..total);
                }
            }
        }
    }
}

/// Склеить init-сегмент и media-сегмент в файл, который читает обычный
/// `Demuxer`. Для WebM заголовок `Segment` собирается с точной длиной.
pub(crate) fn assemble(format: Format, init: &Init, media: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(init.bytes.len() + media.len() + 16);
    match format {
        Format::Mp4 => {
            out.extend_from_slice(&init.bytes);
            out.extend_from_slice(media);
        }
        Format::WebM => {
            let body = &init.bytes[init.body_start..];
            out.extend_from_slice(&init.bytes[..init.body_start]);
            out.extend_from_slice(&[0x18, 0x53, 0x80, 0x67, 0x01]);
            let media = if media.is_empty() { &EMPTY_CLUSTER[..] } else { media };
            let len = (body.len() + media.len()) as u64;
            out.extend_from_slice(&len.to_be_bytes()[1..]);
            out.extend_from_slice(body);
            out.extend_from_slice(media);
        }
    }
    out
}

/// Полная длина элемента с известным размером; неизвестный размер вне `Cluster`
/// принять нечем.
fn known_total(head_len: usize, size: Option<u64>) -> Result<usize, String> {
    let n = size.ok_or("элемент WebM неизвестной длины вне Cluster")?;
    if n > MAX_UNIT {
        return Err("элемент WebM слишком велик".into());
    }
    Ok(head_len + n as usize)
}

/// Заголовок бокса: полный размер и тип. `None` — заголовок ещё не пришёл целиком.
fn box_header(b: &[u8]) -> Result<Option<(usize, [u8; 4])>, String> {
    let Some(h) = b.get(..8) else { return Ok(None) };
    let kind = [h[4], h[5], h[6], h[7]];
    if !kind.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b' ' | b'-' | b'_' | 0xA9)) {
        return Err("в потоке нет боксов ISO BMFF".into());
    }
    let small = u64::from(u32::from_be_bytes([h[0], h[1], h[2], h[3]]));
    let (size, min) = match small {
        0 => return Err("бокс «до конца потока» в MSE не допускается".into()),
        1 => {
            let Some(big) = b.get(8..16) else { return Ok(None) };
            let mut a = [0u8; 8];
            a.copy_from_slice(big);
            (u64::from_be_bytes(a), 16)
        }
        n => (n, 8),
    };
    if size < min || size > MAX_UNIT {
        return Err("некорректный размер бокса".into());
    }
    Ok(Some((size as usize, kind)))
}

/// Идентификатор EBML вместе с битами длины. `None` — не хватает байтов.
fn read_id(b: &[u8]) -> Result<Option<(u32, usize)>, String> {
    let Some(&first) = b.first() else { return Ok(None) };
    let len = first.leading_zeros() as usize + 1;
    if first == 0 || len > 4 {
        return Err("повреждённый идентификатор EBML".into());
    }
    let Some(raw) = b.get(..len) else { return Ok(None) };
    Ok(Some((raw.iter().fold(0u32, |a, &x| (a << 8) | u32::from(x)), len)))
}

/// Размер элемента EBML: `Some(n)`, либо `None` для «неизвестно» (все биты
/// значения — единицы). Внешний `Option` — не хватает байтов.
fn read_size(b: &[u8]) -> Result<Option<(Option<u64>, usize)>, String> {
    let Some(&first) = b.first() else { return Ok(None) };
    if first == 0 {
        return Err("повреждённый размер элемента EBML".into());
    }
    let len = first.leading_zeros() as usize + 1;
    let Some(raw) = b.get(..len) else { return Ok(None) };
    let mut v = u64::from(first) & ((1u64 << (8 - len)) - 1);
    for &x in &raw[1..] {
        v = (v << 8) | u64::from(x);
    }
    let all_ones = (1u64 << (7 * len)) - 1;
    Ok(Some(((v != all_ones).then_some(v), len)))
}

/// Длина содержимого `Cluster` неизвестного размера: до первого элемента
/// первого уровня. `None` — следующий элемент ещё не показался.
fn unknown_cluster_len(body: &[u8]) -> Result<Option<usize>, String> {
    let mut at = 0usize;
    loop {
        let Some((id, id_len)) = read_id(&body[at..])? else { return Ok(None) };
        if LEVEL1.contains(&id) {
            return Ok(Some(at));
        }
        let Some((size, size_len)) = read_size(&body[at + id_len..])? else { return Ok(None) };
        let total = known_total(id_len + size_len, size)?;
        if body.len() < at + total {
            return Ok(None);
        }
        at += total;
    }
}
