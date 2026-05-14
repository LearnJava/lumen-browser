//! Marker-segment reader для JPEG (ISO/IEC 10918-1 §B.1).
//!
//! Маркер — 2 байта `0xFF NN`, где `NN` — тип. Большинство маркеров несут
//! payload: после `NN` идут 2 байта длины (BE, **включая сами 2 байта**),
//! затем тело. SOI, EOI, RSTn и TEM — stand-alone, без длины.
//!
//! Reader накапливает таблицы (`DQT`, `DHT`) и параметры frame-а (`SOF0`,
//! `DRI`) до встречи `SOS`, после чего управление передаётся в `scan` для
//! entropy decode.

use super::huffman::HuffmanTable;

/// Стандартный JPEG zigzag-порядок (ISO/IEC 10918-1 §A.6, fig. A.6).
/// Преобразует индекс в zigzag-последовательности (0..63) в позицию
/// внутри 8×8 блока (row-major, `row*8+col`).
pub const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Ошибки декодирования JPEG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JpegError {
    /// Поток закончился раньше ожидаемого.
    UnexpectedEof,
    /// Файл не начинается с SOI (`FF D8`).
    NoSoi,
    /// Внутри маркер-секции встретился байт ≠ `0xFF` там, где ожидался.
    BadMarkerPrefix(u8),
    /// Маркер не должен встретиться в этом контексте (например, RST вне scan-а).
    UnexpectedMarker(u8),
    /// Поле длины меньше 2 (длина включает сама себя).
    BadSegmentLength(u16),
    /// Маркер frame-а — не SOF0 (Phase 0 поддерживает только baseline DCT).
    UnsupportedSof(u8),
    /// SOF0 указал precision ≠ 8 bit (`P` в §B.2.2).
    UnsupportedPrecision(u8),
    /// Файл объявляет 2 или 4+ компонент (мы поддерживаем только 1 и 3).
    UnsupportedComponentCount(usize),
    /// Sampling factor выходит за пределы 1..=4 (§B.2.2: 4-bit поле).
    BadSamplingFactor { h: u8, v: u8 },
    /// Sampling factors дают MCU больше 10 блоков (заведомо невалидный JPEG).
    OversizedMcu,
    /// Quantization-таблица с индексом не была определена через DQT перед SOS.
    MissingQuantTable(u8),
    /// Huffman-таблица не была определена через DHT перед SOS.
    MissingHuffmanTable { class: u8, id: u8 },
    /// DQT/DHT/SOF0 повторился неконсистентно — длина не совпала с payload.
    BadTableSegment,
    /// В DQT встретилась таблица с `Pq` ≠ 0 (8-bit) и ≠ 1 (16-bit).
    BadQuantPrecision(u8),
    /// DHT сумма BITS даёт > 256 кодов (не canonical) или таблица превышена.
    BadHuffmanCount(usize),
    /// Canonical Huffman codes нарушают Kraft-McMillan (over-subscribed).
    BadHuffmanCodes,
    /// Bit stream закончился до завершения block-а.
    UnexpectedEndOfBitstream,
    /// Decoded symbol size превышает 16 (DC) или AC byte невалиден.
    BadCoefficientSize(u8),
    /// Компонент scan-а ссылается на ID, не объявленный в SOF.
    BadScanComponent(u8),
    /// SOS объявил число компонент scan-а ≠ числу компонент frame-а.
    /// Phase 0 поддерживает только interleaved (Ns == Nf), для grayscale Ns=1.
    UnsupportedScan { ns: u8, nf: u8 },
    /// Block содержит DCT-коэффициент за пределами 0..63 индекса.
    BadCoefficientPosition(usize),
    /// Encoded-data unexpectedly ended (нет EOI или scan не завершился).
    NoEoi,
    /// Размер изображения ноль по одной из осей.
    ZeroDimension,
    /// Не SOF0 в начале frame-а — например, маркер DAC (arithmetic), не поддерживаем.
    UnsupportedFeature(&'static str),
}

impl core::fmt::Display for JpegError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "обрезанный JPEG-поток"),
            Self::NoSoi => write!(f, "не JPEG: SOI (FF D8) не найден в начале"),
            Self::BadMarkerPrefix(b) => write!(f, "ожидался 0xFF, получен {b:#04x}"),
            Self::UnexpectedMarker(m) => write!(f, "неожиданный маркер FF{m:02X}"),
            Self::BadSegmentLength(l) => write!(f, "длина segment-а {l} меньше 2"),
            Self::UnsupportedSof(m) => write!(
                f,
                "не SOF0 (baseline): FF{m:02X} — extended/progressive/lossless не поддерживается"
            ),
            Self::UnsupportedPrecision(p) => write!(f, "sample precision {p} ≠ 8 бит"),
            Self::UnsupportedComponentCount(n) => write!(f, "{n} компонент — поддерживаются только 1 и 3"),
            Self::BadSamplingFactor { h, v } => write!(f, "недопустимый sampling factor H={h} V={v}"),
            Self::OversizedMcu => write!(f, "MCU слишком большой: hmax×vmax > 10"),
            Self::MissingQuantTable(i) => write!(f, "quantization-таблица {i} не определена в DQT"),
            Self::MissingHuffmanTable { class, id } => {
                write!(f, "Huffman-таблица класса {class} id={id} не определена в DHT")
            }
            Self::BadTableSegment => write!(f, "DQT/DHT/SOF0: длина не совпала с payload"),
            Self::BadQuantPrecision(p) => write!(f, "DQT: Pq={p} (ожидалось 0 или 1)"),
            Self::BadHuffmanCount(n) => write!(f, "DHT: {n} кодов не поместятся в 16-битную таблицу"),
            Self::BadHuffmanCodes => write!(f, "DHT: коды нарушают Kraft-McMillan inequality"),
            Self::UnexpectedEndOfBitstream => write!(f, "bit stream закончился внутри блока"),
            Self::BadCoefficientSize(s) => write!(f, "DCT coefficient size {s} > 15"),
            Self::BadScanComponent(c) => write!(f, "SOS ссылается на неизвестный компонент {c}"),
            Self::UnsupportedScan { ns, nf } => write!(
                f,
                "SOS: Ns={ns} ≠ Nf={nf} (non-interleaved сканы не поддерживаются)"
            ),
            Self::BadCoefficientPosition(p) => write!(f, "DCT coefficient position {p} > 63"),
            Self::NoEoi => write!(f, "не найден EOI (FF D9)"),
            Self::ZeroDimension => write!(f, "ширина или высота равна нулю"),
            Self::UnsupportedFeature(s) => write!(f, "не поддерживается: {s}"),
        }
    }
}

impl std::error::Error for JpegError {}

/// Параметры frame-а из SOF0 (ISO/IEC 10918-1 §B.2.2).
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub components: Vec<Component>,
    /// Максимальный horizontal sampling factor по всем компонентам.
    pub h_max: u8,
    /// Максимальный vertical sampling factor по всем компонентам.
    pub v_max: u8,
}

/// Один компонент изображения (§B.2.2 component-specification).
#[derive(Debug, Clone)]
pub struct Component {
    /// Component identifier (Y=1, Cb=2, Cr=3 в JFIF).
    pub id: u8,
    /// Horizontal sampling factor (1..=4).
    pub h_sampling: u8,
    /// Vertical sampling factor (1..=4).
    pub v_sampling: u8,
    /// Индекс quantization-таблицы (`Tq`, 0..=3).
    pub qt_id: u8,
}

/// Компонент scan-а из SOS (§B.2.3).
#[derive(Debug, Clone)]
pub struct ScanComponent {
    /// Индекс в `Frame.components` (не tag id — позиционный).
    pub frame_index: usize,
    /// Huffman DC table id (Td, 0..=3).
    pub dc_table: u8,
    /// Huffman AC table id (Ta, 0..=3).
    pub ac_table: u8,
}

/// Состояние, собранное до начала entropy-coded scan-а.
#[derive(Debug)]
pub struct JpegContext {
    pub frame: Frame,
    pub scan: Vec<ScanComponent>,
    /// Quantization tables, индекс = Tq (0..=3); de-zigzagged в natural row-major.
    pub quant_tables: [Option<[u16; 64]>; 4],
    /// Huffman DC tables (id 0..=3).
    pub dc_tables: [Option<HuffmanTable>; 4],
    /// Huffman AC tables (id 0..=3).
    pub ac_tables: [Option<HuffmanTable>; 4],
    /// Restart interval (`Ri` из DRI) — число MCU между RSTn. 0 = выключен.
    pub restart_interval: u16,
}

/// Reader marker-segments. Хранит положение во входном буфере;
/// после `read_until_scan` положение указывает на первый байт entropy-data.
pub struct SegmentReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> SegmentReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Текущая позиция (для передачи в bit_reader после SOS).
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Доступ к исходному буферу (нужен bit_reader-у).
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Читает 2 байта BE.
    fn read_u16(&mut self) -> Result<u16, JpegError> {
        if self.pos + 2 > self.bytes.len() {
            return Err(JpegError::UnexpectedEof);
        }
        let hi = self.bytes[self.pos];
        let lo = self.bytes[self.pos + 1];
        self.pos += 2;
        Ok((u16::from(hi) << 8) | u16::from(lo))
    }

    fn read_u8(&mut self) -> Result<u8, JpegError> {
        if self.pos >= self.bytes.len() {
            return Err(JpegError::UnexpectedEof);
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Ok(b)
    }

    /// Читает marker (`0xFF NN`). Пропускает fill-байты `0xFF`, разрешённые
    /// между marker-segments по §B.1.1.2.
    fn read_marker(&mut self) -> Result<u8, JpegError> {
        // Первый байт должен быть 0xFF.
        let first = self.read_u8()?;
        if first != 0xFF {
            return Err(JpegError::BadMarkerPrefix(first));
        }
        // Пропускаем подряд идущие 0xFF (fill bytes).
        loop {
            let b = self.read_u8()?;
            if b != 0xFF {
                return Ok(b);
            }
        }
    }

    /// Читает payload фиксированной длины (без поля длины — оно уже разобрано
    /// caller-ом). Возвращает срез внутри буфера.
    fn read_payload(&mut self, len: usize) -> Result<&'a [u8], JpegError> {
        if self.pos + len > self.bytes.len() {
            return Err(JpegError::UnexpectedEof);
        }
        let slice = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    /// Главный entry-point: читает marker-segments от SOI до SOS включительно,
    /// возвращает контекст для entropy decode.
    pub fn read_until_scan(&mut self) -> Result<JpegContext, JpegError> {
        // SOI — два первых байта файла (без fill-bytes префикса).
        if self.bytes.len() < 2 || self.bytes[0] != 0xFF || self.bytes[1] != 0xD8 {
            return Err(JpegError::NoSoi);
        }
        self.pos = 2;

        let mut frame: Option<Frame> = None;
        let scan: Vec<ScanComponent>;
        let mut quant_tables: [Option<[u16; 64]>; 4] = [None, None, None, None];
        let mut dc_tables: [Option<HuffmanTable>; 4] = [None, None, None, None];
        let mut ac_tables: [Option<HuffmanTable>; 4] = [None, None, None, None];
        let mut restart_interval: u16 = 0;

        loop {
            let marker = self.read_marker()?;
            match marker {
                // SOF0 — Baseline DCT.
                0xC0 => {
                    let len = self.read_u16()?;
                    if len < 2 {
                        return Err(JpegError::BadSegmentLength(len));
                    }
                    let payload = self.read_payload(usize::from(len) - 2)?;
                    frame = Some(parse_sof0(payload)?);
                }
                // SOF1..SOF15 кроме SOF4 (DHT) — другие профили, не поддерживаем.
                0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE
                | 0xCF => return Err(JpegError::UnsupportedSof(marker)),
                // DHT — Define Huffman Table.
                0xC4 => {
                    let len = self.read_u16()?;
                    if len < 2 {
                        return Err(JpegError::BadSegmentLength(len));
                    }
                    let payload = self.read_payload(usize::from(len) - 2)?;
                    parse_dht(payload, &mut dc_tables, &mut ac_tables)?;
                }
                // DAC — Define Arithmetic Coding, не поддерживаем.
                0xCC => return Err(JpegError::UnsupportedFeature("arithmetic coding")),
                // DQT — Define Quantization Table.
                0xDB => {
                    let len = self.read_u16()?;
                    if len < 2 {
                        return Err(JpegError::BadSegmentLength(len));
                    }
                    let payload = self.read_payload(usize::from(len) - 2)?;
                    parse_dqt(payload, &mut quant_tables)?;
                }
                // DRI — Define Restart Interval.
                0xDD => {
                    let len = self.read_u16()?;
                    if len != 4 {
                        return Err(JpegError::BadSegmentLength(len));
                    }
                    restart_interval = self.read_u16()?;
                }
                // SOS — Start of Scan. После него — entropy-data, выходим.
                0xDA => {
                    let len = self.read_u16()?;
                    if len < 2 {
                        return Err(JpegError::BadSegmentLength(len));
                    }
                    let payload = self.read_payload(usize::from(len) - 2)?;
                    let f = frame.as_ref().ok_or(JpegError::UnsupportedFeature(
                        "SOS перед SOF0",
                    ))?;
                    scan = parse_sos(payload, f)?;
                    break;
                }
                // APPn / COM — пропускаем payload.
                0xE0..=0xEF | 0xFE => {
                    let len = self.read_u16()?;
                    if len < 2 {
                        return Err(JpegError::BadSegmentLength(len));
                    }
                    let _ = self.read_payload(usize::from(len) - 2)?;
                }
                // DNL — Define Number of Lines. Phase 0 не поддерживает (редкость).
                0xDC => return Err(JpegError::UnsupportedFeature("DNL marker")),
                // EOI до SOS — пустой файл.
                0xD9 => return Err(JpegError::UnexpectedMarker(0xD9)),
                // RSTn — допустимы только внутри entropy-data, не здесь.
                0xD0..=0xD7 => return Err(JpegError::UnexpectedMarker(marker)),
                // 0x00, 0xFF — байт-stuffing/padding в entropy-data, не должен быть здесь.
                0x00 | 0xFF => return Err(JpegError::UnexpectedMarker(marker)),
                // Прочие неизвестные/зарезервированные маркеры — try-skip как APP.
                _ => {
                    let len = self.read_u16()?;
                    if len < 2 {
                        return Err(JpegError::BadSegmentLength(len));
                    }
                    let _ = self.read_payload(usize::from(len) - 2)?;
                }
            }
        }

        let frame = frame.ok_or(JpegError::UnsupportedFeature("frame без SOF0"))?;

        if frame.width == 0 || frame.height == 0 {
            return Err(JpegError::ZeroDimension);
        }

        Ok(JpegContext {
            frame,
            scan,
            quant_tables,
            dc_tables,
            ac_tables,
            restart_interval,
        })
    }
}

/// SOF0 payload: `P H V Nf [Ci Hi/Vi Tqi]×Nf`.
fn parse_sof0(payload: &[u8]) -> Result<Frame, JpegError> {
    if payload.len() < 6 {
        return Err(JpegError::BadTableSegment);
    }
    let precision = payload[0];
    if precision != 8 {
        return Err(JpegError::UnsupportedPrecision(precision));
    }
    let height = (u16::from(payload[1]) << 8) | u16::from(payload[2]);
    let width = (u16::from(payload[3]) << 8) | u16::from(payload[4]);
    let nf = payload[5] as usize;
    if nf != 1 && nf != 3 {
        return Err(JpegError::UnsupportedComponentCount(nf));
    }
    if payload.len() != 6 + 3 * nf {
        return Err(JpegError::BadTableSegment);
    }
    let mut components = Vec::with_capacity(nf);
    let mut h_max = 0u8;
    let mut v_max = 0u8;
    for i in 0..nf {
        let base = 6 + 3 * i;
        let id = payload[base];
        let sampling = payload[base + 1];
        let h_sampling = sampling >> 4;
        let v_sampling = sampling & 0x0F;
        let qt_id = payload[base + 2];
        if !(1..=4).contains(&h_sampling) || !(1..=4).contains(&v_sampling) {
            return Err(JpegError::BadSamplingFactor {
                h: h_sampling,
                v: v_sampling,
            });
        }
        if qt_id > 3 {
            return Err(JpegError::MissingQuantTable(qt_id));
        }
        h_max = h_max.max(h_sampling);
        v_max = v_max.max(v_sampling);
        components.push(Component {
            id,
            h_sampling,
            v_sampling,
            qt_id,
        });
    }
    // Sanity по §B.2.3: сумма Hi×Vi всех компонент в interleaved scan-е
    // не должна превышать 10 (JPEG spec ограничивает MCU). Для 4:2:0
    // имеем 2×2 + 1×1 + 1×1 = 6 — валидный случай.
    let total_blocks: usize = components
        .iter()
        .map(|c| usize::from(c.h_sampling) * usize::from(c.v_sampling))
        .sum();
    if total_blocks > 10 {
        return Err(JpegError::OversizedMcu);
    }
    Ok(Frame {
        width,
        height,
        components,
        h_max,
        v_max,
    })
}

/// DQT payload: повторяющиеся `Pq Tq [q×64 или q×128]`.
fn parse_dqt(payload: &[u8], tables: &mut [Option<[u16; 64]>; 4]) -> Result<(), JpegError> {
    let mut i = 0;
    while i < payload.len() {
        let header = payload[i];
        i += 1;
        let pq = header >> 4;
        let tq = (header & 0x0F) as usize;
        if tq > 3 {
            return Err(JpegError::BadQuantPrecision(header));
        }
        let element_bytes = match pq {
            0 => 1,
            1 => 2,
            other => return Err(JpegError::BadQuantPrecision(other)),
        };
        let table_bytes = 64 * element_bytes;
        if i + table_bytes > payload.len() {
            return Err(JpegError::BadTableSegment);
        }
        let mut table = [0u16; 64];
        for k in 0..64 {
            let v = if pq == 0 {
                u16::from(payload[i + k])
            } else {
                (u16::from(payload[i + 2 * k]) << 8) | u16::from(payload[i + 2 * k + 1])
            };
            // Storage в zigzag-порядке файла → размещаем сразу в natural order.
            table[ZIGZAG[k]] = v;
        }
        tables[tq] = Some(table);
        i += table_bytes;
    }
    if i != payload.len() {
        return Err(JpegError::BadTableSegment);
    }
    Ok(())
}

/// DHT payload: повторяющиеся `Tc/Th [BITS×16] [HUFFVAL×N]`.
fn parse_dht(
    payload: &[u8],
    dc_tables: &mut [Option<HuffmanTable>; 4],
    ac_tables: &mut [Option<HuffmanTable>; 4],
) -> Result<(), JpegError> {
    let mut i = 0;
    while i < payload.len() {
        if i + 17 > payload.len() {
            return Err(JpegError::BadTableSegment);
        }
        let header = payload[i];
        i += 1;
        let class = header >> 4;
        let id = (header & 0x0F) as usize;
        if class > 1 || id > 3 {
            return Err(JpegError::BadHuffmanCount(usize::from(header)));
        }
        let mut bits = [0u8; 16];
        bits.copy_from_slice(&payload[i..i + 16]);
        i += 16;
        let nsymbols: usize = bits.iter().map(|&b| b as usize).sum();
        if nsymbols > 256 {
            return Err(JpegError::BadHuffmanCount(nsymbols));
        }
        if i + nsymbols > payload.len() {
            return Err(JpegError::BadTableSegment);
        }
        let symbols = payload[i..i + nsymbols].to_vec();
        i += nsymbols;
        let table = HuffmanTable::build(bits, symbols)?;
        match class {
            0 => dc_tables[id] = Some(table),
            1 => ac_tables[id] = Some(table),
            _ => unreachable!("class проверен выше"),
        }
    }
    if i != payload.len() {
        return Err(JpegError::BadTableSegment);
    }
    Ok(())
}

/// SOS payload: `Ns [Csj Tdj/Taj]×Ns Ss Se Ah/Al`.
fn parse_sos(payload: &[u8], frame: &Frame) -> Result<Vec<ScanComponent>, JpegError> {
    if payload.is_empty() {
        return Err(JpegError::BadTableSegment);
    }
    let ns = payload[0];
    if usize::from(ns) != frame.components.len() {
        return Err(JpegError::UnsupportedScan {
            ns,
            nf: frame.components.len() as u8,
        });
    }
    let expected_len = 1 + 2 * usize::from(ns) + 3;
    if payload.len() != expected_len {
        return Err(JpegError::BadTableSegment);
    }
    let mut scan = Vec::with_capacity(usize::from(ns));
    for j in 0..usize::from(ns) {
        let cs = payload[1 + 2 * j];
        let td_ta = payload[2 + 2 * j];
        let dc_table = td_ta >> 4;
        let ac_table = td_ta & 0x0F;
        if dc_table > 3 || ac_table > 3 {
            return Err(JpegError::MissingHuffmanTable {
                class: 0,
                id: dc_table.max(ac_table),
            });
        }
        let frame_index = frame
            .components
            .iter()
            .position(|c| c.id == cs)
            .ok_or(JpegError::BadScanComponent(cs))?;
        scan.push(ScanComponent {
            frame_index,
            dc_table,
            ac_table,
        });
    }
    // Ss / Se / Ah-Al для baseline должны быть 0 / 63 / 0; нестрого проверяем,
    // но в нашем case это всегда так — реальные baseline-encoder-ы пишут именно так.
    let ss = payload[1 + 2 * usize::from(ns)];
    let se = payload[2 + 2 * usize::from(ns)];
    if ss != 0 || se != 63 {
        return Err(JpegError::UnsupportedFeature("progressive SOS Ss/Se ≠ 0/63"));
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0xFFD8 (SOI) + 0xFFD9 (EOI) — минимальный валидный (но без frame) JPEG.
    #[test]
    fn empty_jpeg_after_soi_errors_cleanly() {
        let bytes = [0xFF, 0xD8, 0xFF, 0xD9];
        let mut r = SegmentReader::new(&bytes);
        let err = r.read_until_scan().unwrap_err();
        // EOI без SOF0 → UnexpectedMarker(0xD9).
        assert_eq!(err, JpegError::UnexpectedMarker(0xD9));
    }

    #[test]
    fn missing_soi() {
        let bytes = [0x00, 0x00];
        let mut r = SegmentReader::new(&bytes);
        assert_eq!(r.read_until_scan().unwrap_err(), JpegError::NoSoi);
    }

    #[test]
    fn truncated_after_soi() {
        let bytes = [0xFF, 0xD8];
        let mut r = SegmentReader::new(&bytes);
        assert_eq!(r.read_until_scan().unwrap_err(), JpegError::UnexpectedEof);
    }

    #[test]
    fn zigzag_inverse_property() {
        // ZIGZAG — перестановка 0..63.
        let mut seen = [false; 64];
        for &p in &ZIGZAG {
            assert!(!seen[p], "позиция {p} встретилась дважды");
            seen[p] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn dqt_parses_8bit_table() {
        // Pq=0 Tq=0, 64 байта значений 1..=64.
        let mut payload = vec![0x00];
        payload.extend(1..=64u8);
        let mut tables = [None, None, None, None];
        parse_dqt(&payload, &mut tables).unwrap();
        let t = tables[0].unwrap();
        // Первый элемент zigzag = позиция 0 (DC), значение 1.
        assert_eq!(t[0], 1);
        // Второй элемент zigzag = позиция 1 (row 0 col 1), значение 2.
        assert_eq!(t[1], 2);
        // Третий элемент zigzag = позиция 8 (row 1 col 0), значение 3.
        assert_eq!(t[8], 3);
    }

    #[test]
    fn dqt_parses_16bit_table() {
        // Pq=1 Tq=1, 128 байтов (64 × u16 BE), все 0x0001 для простоты.
        let mut payload = vec![0x11];
        for _ in 0..64 {
            payload.push(0x00);
            payload.push(0x01);
        }
        let mut tables = [None, None, None, None];
        parse_dqt(&payload, &mut tables).unwrap();
        let t = tables[1].unwrap();
        assert_eq!(t[0], 1);
    }

    #[test]
    fn sof0_parses_grayscale() {
        // P=8, h=10, w=20, Nf=1, [id=1, H/V=0x11, Tq=0].
        let payload = [8, 0, 10, 0, 20, 1, 1, 0x11, 0];
        let frame = parse_sof0(&payload).unwrap();
        assert_eq!(frame.width, 20);
        assert_eq!(frame.height, 10);
        assert_eq!(frame.components.len(), 1);
        assert_eq!(frame.h_max, 1);
        assert_eq!(frame.v_max, 1);
    }

    #[test]
    fn sof0_parses_rgb_420_subsampling() {
        // YCbCr 4:2:0: Y H/V=2/2, Cb,Cr H/V=1/1.
        let payload = [8, 0, 16, 0, 16, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1];
        let frame = parse_sof0(&payload).unwrap();
        assert_eq!(frame.components.len(), 3);
        assert_eq!(frame.h_max, 2);
        assert_eq!(frame.v_max, 2);
        assert_eq!(frame.components[0].h_sampling, 2);
        assert_eq!(frame.components[0].v_sampling, 2);
        assert_eq!(frame.components[1].qt_id, 1);
    }

    #[test]
    fn sof0_rejects_progressive_via_unsupported_sof() {
        // Тест — что caller-side: SOF2 маркер должен дать UnsupportedSof.
        let bytes = [
            0xFF, 0xD8, // SOI
            0xFF, 0xC2, // SOF2 — progressive
            0x00, 0x08, // длина 8
            0x08, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11, 0x00,
        ];
        let mut r = SegmentReader::new(&bytes);
        assert_eq!(r.read_until_scan().unwrap_err(), JpegError::UnsupportedSof(0xC2));
    }

    #[test]
    fn fill_bytes_between_markers_are_skipped() {
        // 0xFF padding между маркерами разрешён §B.1.1.2.
        let bytes = [
            0xFF, 0xD8, // SOI
            0xFF, 0xFF, 0xFF, 0xC0, // SOF0 после padding-а
            0x00, 0x0B, // длина 11
            0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00,
            0xFF, 0xD9, // EOI (без SOS — но read_until_scan не дочитает)
        ];
        let mut r = SegmentReader::new(&bytes);
        let err = r.read_until_scan().unwrap_err();
        // SOF0 разобрался; следующий маркер EOI до SOS.
        assert_eq!(err, JpegError::UnexpectedMarker(0xD9));
    }

    #[test]
    fn appn_and_comment_are_skipped() {
        let bytes = [
            0xFF, 0xD8, 0xFF, 0xE0, // SOI + APP0
            0x00, 0x06, 0x4A, 0x46, 0x49, 0x46, // длина 6, "JFIF"
            0xFF, 0xFE, // COM
            0x00, 0x05, 0x68, 0x69, 0x21, // длина 5, "hi!"
            0xFF, 0xD9, // EOI без SOF
        ];
        let mut r = SegmentReader::new(&bytes);
        // SOF0 не было → ошибка должна быть про UnexpectedMarker(EOI).
        assert_eq!(r.read_until_scan().unwrap_err(), JpegError::UnexpectedMarker(0xD9));
    }
}
