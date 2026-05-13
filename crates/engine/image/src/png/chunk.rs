//! Парсинг PNG-сигнатуры и итератор чанков с верификацией CRC32.
//!
//! CRC32 — стандартный IEEE 802.3, полином `0xEDB88320` в reflected-форме
//! (старший бит справа). Расчётная таблица из 256 элементов
//! предкомпилирована `const fn` — это не runtime-инициализация, не
//! `LazyLock`, не `static mut`, а просто массив, материализованный
//! компилятором.
//!
//! `ChunkReader` — однопроходный итератор по входному срезу. Он не копирует
//! данные чанков: `Chunk.data` — заёмный срез исходного буфера. CRC
//! проверяется на каждом шаге; первая же ошибка возвращается как
//! `DecodeError::BadCrc { .. }`, и итерация прекращается.

use crate::DecodeError;

/// Длина чанка >= 2^31 запрещена PNG §11.2.2; ограничение задаётся ровно
/// этим значением (а не `u32::MAX`), чтобы любые операции с длинами
/// помещались в `usize` без переполнения на 32-битных платформах.
const MAX_CHUNK_LEN: u32 = 0x7FFF_FFFF;

/// Прочитать PNG-сигнатуру из начала буфера. Возвращает срез после
/// сигнатуры или `InvalidSignature` / `UnexpectedEof`.
pub(crate) fn read_signature(bytes: &[u8]) -> Result<&[u8], DecodeError> {
    if bytes.len() < super::SIGNATURE.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let (sig, rest) = bytes.split_at(super::SIGNATURE.len());
    if sig != super::SIGNATURE {
        return Err(DecodeError::InvalidSignature);
    }
    Ok(rest)
}

/// Чанк PNG. `data` — заёмный срез исходного буфера, копий не делаем.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Chunk<'a> {
    pub kind: [u8; 4],
    pub data: &'a [u8],
}

/// Однопроходный итератор по чанкам. Реализует обычный `next() -> Option<…>`,
/// чтобы можно было управлять контролем потока (например, остановиться
/// на `IEND` вручную).
pub(crate) struct ChunkReader<'a> {
    cursor: &'a [u8],
    done: bool,
}

impl<'a> ChunkReader<'a> {
    pub(crate) fn new(after_signature: &'a [u8]) -> Self {
        Self {
            cursor: after_signature,
            done: false,
        }
    }

    /// Прочитать следующий чанк. `None` означает «вход закончился без ошибки»;
    /// `Some(Err(…))` — повреждённый чанк (короткие данные / CRC).
    pub(crate) fn next_chunk(&mut self) -> Option<Result<Chunk<'a>, DecodeError>> {
        if self.done {
            return None;
        }
        if self.cursor.is_empty() {
            return None;
        }
        Some(self.read_one())
    }

    fn read_one(&mut self) -> Result<Chunk<'a>, DecodeError> {
        if self.cursor.len() < 8 {
            self.done = true;
            return Err(DecodeError::UnexpectedEof);
        }
        let len = u32::from_be_bytes(self.cursor[0..4].try_into().unwrap());
        if len > MAX_CHUNK_LEN {
            self.done = true;
            return Err(DecodeError::ChunkTooLong { len });
        }
        let kind: [u8; 4] = self.cursor[4..8].try_into().unwrap();
        let total_after_header = 4 + len as usize;
        if self.cursor.len() < 8 + total_after_header {
            self.done = true;
            return Err(DecodeError::UnexpectedEof);
        }
        let data = &self.cursor[8..8 + len as usize];
        let crc_start = 8 + len as usize;
        let expected_crc = u32::from_be_bytes(
            self.cursor[crc_start..crc_start + 4]
                .try_into()
                .unwrap(),
        );

        let mut hasher = Crc32::new();
        hasher.update(&kind);
        hasher.update(data);
        let actual_crc = hasher.finish();
        if actual_crc != expected_crc {
            self.done = true;
            return Err(DecodeError::BadCrc {
                chunk_type: kind,
                expected: expected_crc,
                actual: actual_crc,
            });
        }

        self.cursor = &self.cursor[crc_start + 4..];
        Ok(Chunk { kind, data })
    }
}

/// CRC32 (IEEE 802.3, reflected). Используется PNG в чанках и
/// дублируется как самостоятельная функция для будущих форматов.
pub(crate) struct Crc32 {
    state: u32,
}

impl Crc32 {
    pub(crate) const fn new() -> Self {
        Self { state: !0u32 }
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        let mut c = self.state;
        for &b in bytes {
            let idx = ((c ^ u32::from(b)) & 0xFF) as usize;
            c = TABLE[idx] ^ (c >> 8);
        }
        self.state = c;
    }

    pub(crate) const fn finish(&self) -> u32 {
        !self.state
    }
}

/// Удобная свободная функция: CRC32 одного среза за один вызов. Полезна в
/// тестах, где не нужен инкрементный API.
#[cfg(test)]
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut h = Crc32::new();
    h.update(bytes);
    h.finish()
}

const TABLE: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut c = i;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i as usize] = c;
        i += 1;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecodeError;

    #[test]
    fn crc32_known_vectors() {
        // Известные тестовые векторы CRC32 (IEEE 802.3 reflected).
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn crc32_incremental_matches_single_shot() {
        let s = b"The quick brown fox jumps over the lazy dog";
        let one_shot = crc32(s);
        let mut h = Crc32::new();
        h.update(&s[..10]);
        h.update(&s[10..20]);
        h.update(&s[20..]);
        assert_eq!(h.finish(), one_shot);
    }

    #[test]
    fn signature_ok() {
        let buf = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0xAA, 0xBB,
        ];
        let rest = read_signature(&buf).unwrap();
        assert_eq!(rest, &[0xAA, 0xBB]);
    }

    #[test]
    fn signature_wrong_magic() {
        let buf = [0; 8];
        assert_eq!(read_signature(&buf), Err(DecodeError::InvalidSignature));
    }

    #[test]
    fn signature_too_short() {
        let buf = [0x89, 0x50, 0x4E, 0x47];
        assert_eq!(read_signature(&buf), Err(DecodeError::UnexpectedEof));
    }

    fn build_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut h = Crc32::new();
        h.update(kind);
        h.update(data);
        out.extend_from_slice(&h.finish().to_be_bytes());
        out
    }

    #[test]
    fn chunk_reader_single() {
        let chunk = build_chunk(b"IEND", &[]);
        let mut r = ChunkReader::new(&chunk);
        let c = r.next_chunk().unwrap().unwrap();
        assert_eq!(&c.kind, b"IEND");
        assert!(c.data.is_empty());
        assert!(r.next_chunk().is_none());
    }

    #[test]
    fn chunk_reader_multiple() {
        let mut buf = Vec::new();
        buf.extend(build_chunk(b"IHDR", &[1, 2, 3]));
        buf.extend(build_chunk(b"IDAT", &[4, 5, 6, 7, 8]));
        buf.extend(build_chunk(b"IEND", &[]));
        let mut r = ChunkReader::new(&buf);
        let a = r.next_chunk().unwrap().unwrap();
        assert_eq!(&a.kind, b"IHDR");
        assert_eq!(a.data, &[1, 2, 3]);
        let b = r.next_chunk().unwrap().unwrap();
        assert_eq!(&b.kind, b"IDAT");
        assert_eq!(b.data, &[4, 5, 6, 7, 8]);
        let c = r.next_chunk().unwrap().unwrap();
        assert_eq!(&c.kind, b"IEND");
        assert!(r.next_chunk().is_none());
    }

    #[test]
    fn chunk_reader_bad_crc() {
        let mut chunk = build_chunk(b"IHDR", &[1, 2, 3]);
        let len = chunk.len();
        chunk[len - 1] ^= 0xFF; // ломаем последний байт CRC
        let mut r = ChunkReader::new(&chunk);
        match r.next_chunk() {
            Some(Err(DecodeError::BadCrc { chunk_type, .. })) => {
                assert_eq!(&chunk_type, b"IHDR");
            }
            other => panic!("ожидалась BadCrc, получено {other:?}"),
        }
        // После ошибки итератор останавливается.
        assert!(r.next_chunk().is_none());
    }

    #[test]
    fn chunk_reader_truncated_data() {
        // length = 5, type IDAT, но самих байтов нет.
        let buf = [
            0, 0, 0, 5, b'I', b'D', b'A', b'T',
            // пропускаем 5 байтов данных и 4 байта CRC
        ];
        let mut r = ChunkReader::new(&buf);
        assert!(matches!(
            r.next_chunk(),
            Some(Err(DecodeError::UnexpectedEof))
        ));
    }

    #[test]
    fn chunk_reader_too_long_length() {
        let buf = [
            0xFF, 0xFF, 0xFF, 0xFF, b'I', b'D', b'A', b'T',
        ];
        let mut r = ChunkReader::new(&buf);
        assert!(matches!(
            r.next_chunk(),
            Some(Err(DecodeError::ChunkTooLong { .. }))
        ));
    }
}
