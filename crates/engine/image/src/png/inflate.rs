//! DEFLATE / zlib декодер по RFC 1951 + RFC 1950.
//!
//! Реализуется без сторонних крейтов: ни `miniz_oxide`, ни `flate2`,
//! ни `inflate` (см. §5 политики зависимостей). Свой decoder бьётся ровно
//! по специфике PNG-потоков:
//! - целевой сценарий — расжатие конкатенированных `IDAT`-чанков, размер
//!   на типичной странице порядка КБ-МБ, не GB-streaming-сценарии;
//! - алгоритм не паникует, любая ошибка — `InflateError`;
//! - allocations bounded: три маленьких декодера Huffman'а + output Vec.
//!
//! Архитектура простая:
//! - `BitReader` читает биты LSB-first (формат DEFLATE).
//! - `HuffmanDecoder` — каноническая таблица code-lengths по RFC 1951
//!   §3.2.2, decode идёт битом-за-битом через ranges-of-codes.
//! - `inflate_zlib` — главная функция: zlib header (RFC 1950 §2.2),
//!   последовательность DEFLATE-блоков, проверка adler-32.
//!
//! Поддержаны все три типа блоков:
//! - `00` stored (uncompressed) — байт-копия после выравнивания.
//! - `01` fixed Huffman — литералы/длины 7/8/9 бит по RFC 1951 §3.2.6,
//!   distance — 5 бит.
//! - `10` dynamic Huffman — параметры в начале блока, code-length-codes,
//!   потом lit/len + dist.

use crate::InflateError;

/// Расжать zlib-поток (RFC 1950): 2-байтовый header (CMF+FLG) + DEFLATE-
/// данные + 4 байта adler-32 в big-endian.
pub(crate) fn inflate_zlib(input: &[u8]) -> Result<Vec<u8>, InflateError> {
    if input.len() < 6 {
        return Err(InflateError::BadZlibHeader);
    }
    // CMF / FLG проверка: CM (низшие 4 бита CMF) должно быть 8 = deflate;
    // (CMF << 8 | FLG) делимо на 31 (RFC 1950 §2.2). FDICT (бит 5 FLG)
    // должен быть 0 — словарь preset не предусмотрен PNG'ом.
    let cmf = input[0];
    let flg = input[1];
    let cm = cmf & 0x0F;
    if cm != 8 {
        return Err(InflateError::BadZlibHeader);
    }
    let check = (u16::from(cmf) << 8) | u16::from(flg);
    if check % 31 != 0 {
        return Err(InflateError::BadZlibHeader);
    }
    let fdict = (flg >> 5) & 1;
    if fdict != 0 {
        return Err(InflateError::BadZlibHeader);
    }

    let deflate_end = input.len() - 4;
    let mut br = BitReader::new(&input[2..deflate_end]);
    let mut out: Vec<u8> = Vec::new();
    inflate_blocks(&mut br, &mut out)?;

    let expected_adler = u32::from_be_bytes(input[deflate_end..].try_into().unwrap());
    let actual_adler = adler32(&out);
    if expected_adler != actual_adler {
        return Err(InflateError::BadAdler32 {
            expected: expected_adler,
            actual: actual_adler,
        });
    }
    Ok(out)
}

fn inflate_blocks(br: &mut BitReader<'_>, out: &mut Vec<u8>) -> Result<(), InflateError> {
    loop {
        let bfinal = br.read_bits(1)?;
        let btype = br.read_bits(2)?;
        match btype {
            0 => inflate_stored(br, out)?,
            1 => inflate_huffman(br, out, &fixed_lit_len(), &fixed_dist())?,
            2 => {
                let (lit_len, dist) = read_dynamic_tables(br)?;
                inflate_huffman(br, out, &lit_len, &dist)?;
            }
            _ => return Err(InflateError::ReservedBlockType),
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(())
}

fn inflate_stored(br: &mut BitReader<'_>, out: &mut Vec<u8>) -> Result<(), InflateError> {
    br.align_to_byte();
    let len_lo = br.read_byte()?;
    let len_hi = br.read_byte()?;
    let nlen_lo = br.read_byte()?;
    let nlen_hi = br.read_byte()?;
    let len = u16::from(len_lo) | (u16::from(len_hi) << 8);
    let nlen = u16::from(nlen_lo) | (u16::from(nlen_hi) << 8);
    if len != !nlen {
        return Err(InflateError::BadStoredLength);
    }
    out.reserve(len as usize);
    for _ in 0..len {
        out.push(br.read_byte()?);
    }
    Ok(())
}

fn inflate_huffman(
    br: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    lit_len: &HuffmanDecoder,
    dist: &HuffmanDecoder,
) -> Result<(), InflateError> {
    loop {
        let sym = lit_len.decode(br)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let length = decode_length(sym, br)?;
                let dist_sym = dist.decode(br)?;
                if dist_sym >= 30 {
                    return Err(InflateError::BadLengthOrDistanceCode);
                }
                let distance = decode_distance(dist_sym, br)?;
                copy_lz77(out, distance, length)?;
            }
            _ => return Err(InflateError::BadLengthOrDistanceCode),
        }
    }
}

fn copy_lz77(out: &mut Vec<u8>, distance: usize, length: usize) -> Result<(), InflateError> {
    if distance == 0 || distance > out.len() {
        return Err(InflateError::DistanceTooFar);
    }
    out.reserve(length);
    // distance < length — нужна побайтовая копия (overlapping LZ77):
    // например, distance=1 length=5 повторит последний байт 5 раз.
    for _ in 0..length {
        let src = out[out.len() - distance];
        out.push(src);
    }
    Ok(())
}

// RFC 1951 §3.2.5: length codes 257..=285. Базовая длина и extra-bits
// по таблице. Code 285 — длина 258 без extra-bits (исключение).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

fn decode_length(sym: u16, br: &mut BitReader<'_>) -> Result<usize, InflateError> {
    let idx = (sym - 257) as usize;
    let base = LENGTH_BASE[idx];
    let extra_bits = LENGTH_EXTRA[idx];
    let extra = br.read_bits(extra_bits)?;
    Ok(base as usize + extra as usize)
}

const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn decode_distance(sym: u16, br: &mut BitReader<'_>) -> Result<usize, InflateError> {
    let idx = sym as usize;
    let base = DIST_BASE[idx];
    let extra_bits = DIST_EXTRA[idx];
    let extra = br.read_bits(extra_bits)?;
    Ok(base as usize + extra as usize)
}

// Фиксированные таблицы Huffman по RFC 1951 §3.2.6.
fn fixed_lit_len() -> HuffmanDecoder {
    let mut lens = [0u8; 288];
    for sym in 0u16..=287 {
        lens[sym as usize] = match sym {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            280..=287 => 8,
            _ => unreachable!(),
        };
    }
    HuffmanDecoder::build(&lens).expect("fixed lit/len таблица RFC-1951 валидна")
}

fn fixed_dist() -> HuffmanDecoder {
    // Все 30 distance-кодов длиной 5 бит. (Коды 30 и 31 формально
    // существуют, но недопустимы в decoded-потоке — у нас уже есть
    // проверка `if dist_sym >= 30` выше.)
    let lens = [5u8; 32];
    HuffmanDecoder::build(&lens).expect("fixed dist таблица RFC-1951 валидна")
}

// Динамические Huffman-таблицы: парсинг header'а блока BTYPE=10.
fn read_dynamic_tables(
    br: &mut BitReader<'_>,
) -> Result<(HuffmanDecoder, HuffmanDecoder), InflateError> {
    let hlit = br.read_bits(5)? as usize + 257;
    let hdist = br.read_bits(5)? as usize + 1;
    let hclen = br.read_bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err(InflateError::BadHuffmanCodes);
    }

    // 19 code-length-codes в фиксированном порядке (RFC 1951 §3.2.7).
    const CL_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut cl_lens = [0u8; 19];
    for i in 0..hclen {
        cl_lens[CL_ORDER[i]] = br.read_bits(3)? as u8;
    }
    let cl_decoder = HuffmanDecoder::build(&cl_lens)?;

    // Декодировать hlit + hdist значений code-length, с поддержкой
    // re-run кодов 16/17/18.
    let total = hlit + hdist;
    let mut all_lens = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = cl_decoder.decode(br)?;
        match sym {
            0..=15 => {
                all_lens[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err(InflateError::BadHuffmanCodes);
                }
                let repeat = br.read_bits(2)? as usize + 3;
                let prev = all_lens[i - 1];
                for _ in 0..repeat {
                    if i >= total {
                        return Err(InflateError::BadHuffmanCodes);
                    }
                    all_lens[i] = prev;
                    i += 1;
                }
            }
            17 => {
                let repeat = br.read_bits(3)? as usize + 3;
                for _ in 0..repeat {
                    if i >= total {
                        return Err(InflateError::BadHuffmanCodes);
                    }
                    all_lens[i] = 0;
                    i += 1;
                }
            }
            18 => {
                let repeat = br.read_bits(7)? as usize + 11;
                for _ in 0..repeat {
                    if i >= total {
                        return Err(InflateError::BadHuffmanCodes);
                    }
                    all_lens[i] = 0;
                    i += 1;
                }
            }
            _ => return Err(InflateError::BadHuffmanCodes),
        }
    }
    let lit_len = HuffmanDecoder::build(&all_lens[..hlit])?;
    let dist = HuffmanDecoder::build(&all_lens[hlit..])?;
    Ok((lit_len, dist))
}

const MAX_BITS: usize = 15;

/// Каноническое Huffman-декодирование по code-lengths (RFC 1951 §3.2.2).
///
/// Сериализация символов: внутри `sorted_symbols` хранятся все символы с
/// code_length != 0, отсортированные по `(length, symbol_value)`. Для каждой
/// длины `L` известно начало диапазона (`offset[L]`), количество (`count[L]`)
/// и значение первого канонического кода (`first_code[L]`, left-justified
/// до `L` бит).
///
/// Декодирование: читаем биты по одному; на каждом шаге проверяем, не
/// попал ли текущий накопленный `code` в диапазон `[first_code[L],
/// first_code[L] + count[L])` при той же длине. Если да — символ найден.
pub(crate) struct HuffmanDecoder {
    sorted_symbols: Vec<u16>,
    offset: [usize; MAX_BITS + 1],
    count: [u16; MAX_BITS + 1],
    first_code: [u32; MAX_BITS + 1],
}

impl HuffmanDecoder {
    pub(crate) fn build(lengths: &[u8]) -> Result<Self, InflateError> {
        // 288 — максимум для фиксированной lit/len-таблицы (RFC 1951 §3.2.6);
        // динамические таблицы лимитированы 286 и проверяются в read_dynamic_tables.
        if lengths.len() > 288 {
            return Err(InflateError::BadHuffmanCodes);
        }
        let mut count = [0u16; MAX_BITS + 1];
        for &l in lengths {
            if l as usize > MAX_BITS {
                return Err(InflateError::BadHuffmanCodes);
            }
            if l != 0 {
                count[l as usize] += 1;
            }
        }
        // Проверка Kraft-McMillan: сумма 2^(-L) для каждого занятого кода
        // не должна превышать 1. Эквивалентно: для каждой длины,
        // first_code[L] <= 2^L.
        let mut first_code = [0u32; MAX_BITS + 1];
        let mut code: u32 = 0;
        for l in 1..=MAX_BITS {
            code = (code + u32::from(count[l - 1])) << 1;
            first_code[l] = code;
            if code + u32::from(count[l]) > (1u32 << l) {
                return Err(InflateError::BadHuffmanCodes);
            }
        }

        // Если ни одного кода нет (все lengths == 0), это валидно для
        // distance-таблицы в потоках, где distance не нужен.
        let total_codes: u32 = count.iter().map(|&c| u32::from(c)).sum();

        // Заполнить sorted_symbols. offset[L] = индекс начала символов длины L.
        let mut offset = [0usize; MAX_BITS + 1];
        let mut acc = 0usize;
        for l in 1..=MAX_BITS {
            offset[l] = acc;
            acc += count[l] as usize;
        }
        let mut sorted_symbols = vec![0u16; acc];
        let mut cursor = offset;
        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let pos = cursor[len as usize];
                sorted_symbols[pos] = u16::try_from(sym).map_err(|_| InflateError::BadHuffmanCodes)?;
                cursor[len as usize] = pos + 1;
            }
        }

        // Sanity: если есть ровно один код, он должен быть длиной >= 1 (валиден).
        // Если total_codes == 0, decode никогда не должен вызываться — это
        // ловится в decode через UnexpectedEndOfBitstream / BadHuffmanCodes.
        let _ = total_codes;

        Ok(Self {
            sorted_symbols,
            offset,
            count,
            first_code,
        })
    }

    pub(crate) fn decode(&self, br: &mut BitReader<'_>) -> Result<u16, InflateError> {
        let mut code: u32 = 0;
        for len in 1..=MAX_BITS {
            code = (code << 1) | br.read_bits(1)?;
            let first = self.first_code[len];
            let count = u32::from(self.count[len]);
            if code >= first && code < first + count {
                let idx = self.offset[len] + (code - first) as usize;
                return Ok(self.sorted_symbols[idx]);
            }
        }
        Err(InflateError::BadHuffmanCodes)
    }
}

/// LSB-first bit reader. DEFLATE упаковывает биты так, что младший бит
/// каждого байта — это первый прочитанный бит. Многобитовые значения
/// собираются с младших в старшие.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    pub(crate) fn read_bits(&mut self, n: u8) -> Result<u32, InflateError> {
        debug_assert!(n <= 16, "read_bits принимает не более 16 бит за раз");
        let mut out = 0u32;
        for i in 0..n {
            if self.byte_pos >= self.data.len() {
                return Err(InflateError::UnexpectedEndOfBitstream);
            }
            let bit = (self.data[self.byte_pos] >> self.bit_pos) & 1;
            out |= u32::from(bit) << i;
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }
        Ok(out)
    }

    pub(crate) fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    pub(crate) fn read_byte(&mut self) -> Result<u8, InflateError> {
        debug_assert_eq!(self.bit_pos, 0, "read_byte вызывается только после align_to_byte");
        if self.byte_pos >= self.data.len() {
            return Err(InflateError::UnexpectedEndOfBitstream);
        }
        let b = self.data[self.byte_pos];
        self.byte_pos += 1;
        Ok(b)
    }
}

/// Adler-32 чек-сумма (RFC 1950 §9). Хранится в zlib-трейлере.
fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &b in data {
        s1 = (s1 + u32::from(b)) % MOD_ADLER;
        s2 = (s2 + s1) % MOD_ADLER;
    }
    (s2 << 16) | s1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_reader_reads_lsb_first() {
        // 0b10110010 → биты LSB→MSB: 0,1,0,0,1,1,0,1
        let mut br = BitReader::new(&[0b1011_0010]);
        assert_eq!(br.read_bits(1).unwrap(), 0);
        assert_eq!(br.read_bits(1).unwrap(), 1);
        assert_eq!(br.read_bits(2).unwrap(), 0b00); // следующие два бита
        assert_eq!(br.read_bits(4).unwrap(), 0b1011);
    }

    #[test]
    fn bit_reader_crosses_byte_boundary() {
        // Два байта 0xAB 0xCD = 10101011 11001101.
        // LSB-поток (как читает DEFLATE): 1,1,0,1,0,1,0,1, 1,0,1,1,0,0,1,1.
        let mut br = BitReader::new(&[0xAB, 0xCD]);
        // первые 5 бит: 1,1,0,1,0 → LSB-собрано 0b01011
        assert_eq!(br.read_bits(5).unwrap(), 0b01011);
        // следующие 8 бит: 1,0,1,1,0,1,1,0 → LSB-собрано
        //   1 | 0<<1 | 1<<2 | 1<<3 | 0<<4 | 1<<5 | 1<<6 | 0<<7 = 109
        assert_eq!(br.read_bits(8).unwrap(), 109);
        // последние 3 бита: 0,1,1 → 0b110
        assert_eq!(br.read_bits(3).unwrap(), 0b110);
    }

    #[test]
    fn bit_reader_aligns_to_byte() {
        let mut br = BitReader::new(&[0xAB, 0xCD]);
        let _ = br.read_bits(3).unwrap();
        br.align_to_byte();
        assert_eq!(br.read_byte().unwrap(), 0xCD);
    }

    #[test]
    fn bit_reader_underflow() {
        let mut br = BitReader::new(&[0]);
        let _ = br.read_bits(8);
        assert_eq!(
            br.read_bits(1),
            Err(InflateError::UnexpectedEndOfBitstream)
        );
    }

    #[test]
    fn adler32_known_vectors() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"a"), 0x0062_0062);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn huffman_build_and_decode() {
        // Пример из RFC 1951 §3.2.2: символы A B C D с длинами 2 3 3 2.
        // Канонический код: A=00, D=01, B=100, C=101.
        // lengths по индексу символа: [2,3,3,2]
        let dec = HuffmanDecoder::build(&[2, 3, 3, 2]).unwrap();
        // Декодирование 00 → A=0, 01 → D=3, 100 → B=1, 101 → C=2.
        // Биты пакуются LSB-first per DEFLATE — но HuffmanDecoder читает
        // их в порядке поступления, без переворота, и собирает в `code`
        // как (code << 1) | bit. То есть «00» здесь означает «два нулевых
        // бита подряд» — порядок в потоке.
        let mut br = BitReader::new(&[0b00010100]); // LSB: 0,0,1,0,1,0,0,0
        // первые два бита 0,0 → code=00 → symbol A=0
        assert_eq!(dec.decode(&mut br).unwrap(), 0);
        // следующие два бита 1,0 → code=10 → не диапазон длины 2
        // (count[2]=2, first_code[2]=0, range [0,2)); пробуем длину 3:
        // code=10 << 1 | next_bit(1) = 101 → C=2
        assert_eq!(dec.decode(&mut br).unwrap(), 2);
    }

    #[test]
    fn huffman_oversubscribed_fails() {
        // Слишком много кодов на короткой длине: 3 кода длины 1 = 3 > 2.
        assert!(matches!(
            HuffmanDecoder::build(&[1, 1, 1]),
            Err(InflateError::BadHuffmanCodes)
        ));
    }

    #[test]
    fn huffman_too_long_fails() {
        // Длина 16 запрещена (MAX_BITS=15).
        assert!(matches!(
            HuffmanDecoder::build(&[16]),
            Err(InflateError::BadHuffmanCodes)
        ));
    }

    #[test]
    fn inflate_zlib_stored_block() {
        // Соберём поток: zlib header (CMF=0x78, FLG=0x01, check 31|0).
        // Один stored-блок: BFINAL=1 BTYPE=00 (3 бита: 00000001 LSB →
        // байт 0x01), потом выравнивание, LEN/NLEN, данные.
        // Adler-32 от данных "Hi" = 0x008c0067 (LE на дисплее, BE в потоке).
        let payload = b"Hi";
        let adler = adler32(payload);
        let mut stream = Vec::new();
        stream.push(0x78); // CMF: CINFO=7 (32K окно), CM=8
        stream.push(0x01); // FLG: FCHECK such that (0x78<<8|FLG) % 31 == 0
        stream.push(0x01); // BFINAL=1, BTYPE=00 (биты 1,0,0 LSB → 0x01)
        stream.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes()); // LEN
        let nlen = !u16::try_from(payload.len()).unwrap();
        stream.extend_from_slice(&nlen.to_le_bytes());
        stream.extend_from_slice(payload);
        stream.extend_from_slice(&adler.to_be_bytes());

        let out = inflate_zlib(&stream).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn inflate_zlib_fixed_huffman_hello() {
        // Известный zlib-поток для "Hello", произведённый zlib по умолчанию
        // (fixed Huffman). Префикс 78 9C — стандартный zlib header.
        // Это конкретное представление можно проверить, сжав "Hello"
        // через zlib.compress и сверив байты:
        //   78 9C F3 48 CD C9 C9 07 00 05 8C 01 F5
        let stream: [u8; 13] = [
            0x78, 0x9C, 0xF3, 0x48, 0xCD, 0xC9, 0xC9, 0x07, 0x00, 0x05, 0x8C, 0x01, 0xF5,
        ];
        let out = inflate_zlib(&stream).unwrap();
        assert_eq!(&out, b"Hello");
    }

    #[test]
    fn inflate_zlib_bad_header() {
        // CMF.CM=7 (не deflate) — отклоняем.
        let bad = [0x77, 0x01, 0, 0, 0, 0];
        assert_eq!(inflate_zlib(&bad), Err(InflateError::BadZlibHeader));
    }

    #[test]
    fn inflate_zlib_bad_adler() {
        let payload = b"Hi";
        let mut stream = Vec::new();
        stream.push(0x78);
        stream.push(0x01);
        stream.push(0x01);
        stream.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes());
        let nlen = !u16::try_from(payload.len()).unwrap();
        stream.extend_from_slice(&nlen.to_le_bytes());
        stream.extend_from_slice(payload);
        // Намеренно неправильный adler.
        stream.extend_from_slice(&0u32.to_be_bytes());
        assert!(matches!(
            inflate_zlib(&stream),
            Err(InflateError::BadAdler32 { .. })
        ));
    }

    #[test]
    fn inflate_zlib_dynamic_huffman() {
        // Длинный повторяющийся текст с разной частотой символов вынуждает
        // zlib выбрать BTYPE=10 (dynamic Huffman). Поток сгенерирован
        // Python'ом: zlib.compress(text, 9), где text — 4000 байт смеси
        // трёх повторяющихся фраз. Этот тест покрывает code-length-codes,
        // re-run коды 16/17/18 и кросс-блочный LZ77 — то, что не достижимо
        // через fixed Huffman.
        let stream: [u8; 155] = [
            0x78, 0xDA, 0xED, 0xD4, 0xCB, 0x15, 0x02, 0x21, 0x0C, 0x46, 0xE1, 0x56, 0xFE, 0x0A,
            0xEC, 0x09, 0x21, 0x33, 0x46, 0x81, 0x20, 0x04, 0xC7, 0xB1, 0x7A, 0xE3, 0xD4, 0xE0,
            0x46, 0x4F, 0x56, 0x3C, 0xC2, 0x77, 0x97, 0xE8, 0x85, 0x70, 0x9F, 0x1C, 0x6F, 0x38,
            0x77, 0xD9, 0x2A, 0x16, 0x79, 0xE2, 0x3A, 0x4B, 0xA3, 0x04, 0x79, 0x50, 0x87, 0xDA,
            0x3C, 0x87, 0xD7, 0x8E, 0x6E, 0x37, 0x49, 0xD6, 0xD3, 0x71, 0xE3, 0xC2, 0x85, 0x8B,
            0x5F, 0x12, 0x55, 0x36, 0xF0, 0x38, 0x06, 0xCA, 0x85, 0x4C, 0x74, 0x84, 0x9C, 0xB1,
            0x8A, 0x24, 0x14, 0xAA, 0x50, 0x41, 0x14, 0x1B, 0xD8, 0xFA, 0x79, 0x14, 0xD8, 0x52,
            0xCB, 0xB1, 0x6D, 0xA1, 0xEB, 0xEE, 0x05, 0x2F, 0x78, 0xC1, 0x0B, 0xFF, 0x5E, 0xC8,
            0xD2, 0xA9, 0x80, 0xDB, 0x98, 0xC5, 0x3E, 0x4E, 0x3B, 0x61, 0xB0, 0x22, 0x14, 0x52,
            0x83, 0x75, 0x50, 0x54, 0xD2, 0x69, 0xD1, 0xC4, 0x8D, 0x47, 0xE4, 0xBA, 0x82, 0x32,
            0xAB, 0x3B, 0x77, 0xEE, 0xDC, 0xB9, 0x73, 0xF7, 0x5D, 0xF7, 0x06, 0xB9, 0x53, 0xA7,
            0x90,
        ];
        let mut expected: Vec<u8> = Vec::new();
        expected.extend(b"the quick brown fox jumped over the lazy red dog. ".repeat(20));
        expected.extend(
            b"now is the time for all good men to come to the aid of the party. ".repeat(20),
        );
        expected.extend(b"lorem ipsum dolor sit amet consectetur adipiscing elit. ".repeat(30));
        let out = inflate_zlib(&stream).unwrap();
        assert_eq!(out.len(), expected.len());
        assert_eq!(out, expected);
    }

    #[test]
    fn copy_lz77_overlap_repeats_byte() {
        // Стандартный сценарий «distance=1, length=N» — повтор последнего байта.
        let mut out = vec![b'X'];
        copy_lz77(&mut out, 1, 5).unwrap();
        assert_eq!(out, b"XXXXXX");
    }

    #[test]
    fn copy_lz77_rejects_distance_past_start() {
        let mut out = vec![b'X'];
        assert!(matches!(
            copy_lz77(&mut out, 2, 1),
            Err(InflateError::DistanceTooFar)
        ));
    }
}
