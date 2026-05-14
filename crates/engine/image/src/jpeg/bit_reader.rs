//! Bit reader для entropy-coded JPEG-данных (ISO/IEC 10918-1 §F.2.2).
//!
//! MSB-first битовый поток поверх байтового. Главная особенность —
//! **byte-stuffing**: внутри scan-данных любой `0xFF` экранируется как
//! `0xFF 0x00`. Reader при чтении байта `0xFF` смотрит следующий байт:
//! - `0x00` → возвращает `0xFF` как обычный data-byte и пропускает `0x00`;
//! - всё остальное → это маркер (RSTn / EOI / DNL и пр.); reader сохраняет
//!   его в `marker` поле и больше не пополняет buffer до явного
//!   `consume_marker`.
//!
//! Использование при decode:
//! 1. `read_bits(n)` — следующие n битов как u16.
//! 2. После каждого MCU caller проверяет `peek_marker()`. На RSTm —
//!    `consume_marker()`, выравнивает позицию по байту, сбрасывает DC
//!    predictors (это уже забота scan-loop-а).

use super::marker::JpegError;

pub struct JpegBitReader<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Bit buffer (MSB первый). Хранит до 32 битов; младшие `buffer_bits`
    /// битов справа — валидные.
    buffer: u32,
    buffer_bits: u8,
    /// Если encountered `0xFF NN` с `NN != 0x00` — здесь лежит `NN`.
    /// Пока marker не consumed, refill не пытается читать дальше.
    marker: Option<u8>,
}

impl<'a> JpegBitReader<'a> {
    pub fn new(bytes: &'a [u8], start: usize) -> Self {
        Self {
            bytes,
            pos: start,
            buffer: 0,
            buffer_bits: 0,
            marker: None,
        }
    }

    /// Текущая байтовая позиция (включая все consumed-байты, в т.ч. stuff-нули).
    #[cfg(test)]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Если внутри bit-stream-а встретился маркер — возвращает его (без `0xFF`).
    /// Используется тестами; production-код идёт через `read_restart_marker`.
    #[cfg(test)]
    pub fn peek_marker(&self) -> Option<u8> {
        self.marker
    }

    /// Сбрасывает marker-флаг и выравнивает bit-buffer по байтовой границе.
    /// Используется тестами; production-код идёт через `read_restart_marker`.
    #[cfg(test)]
    pub fn consume_marker(&mut self) {
        self.marker = None;
        self.buffer = 0;
        self.buffer_bits = 0;
    }

    /// Возвращает следующий marker, корректно обрабатывая случай, когда
    /// refill ещё не дошёл до него (буфер был не пуст). На границе restart-
    /// интервала JPEG §F.1.2.3 фиксирует: после последнего МCU добавляются
    /// fill-биты до байтовой границы и затем идёт `FF Dn`. Метод:
    /// 1) выравнивает буфер по байту (сбрасывает остатки fill-битов);
    /// 2) если refill уже видел marker — возвращает его и consume-ит;
    /// 3) иначе читает `FF Dn` из потока напрямую.
    pub fn read_restart_marker(&mut self) -> Result<u8, super::marker::JpegError> {
        self.buffer = 0;
        self.buffer_bits = 0;
        if let Some(m) = self.marker.take() {
            return Ok(m);
        }
        if self.pos + 2 > self.bytes.len() {
            return Err(super::marker::JpegError::UnexpectedEndOfBitstream);
        }
        let ff = self.bytes[self.pos];
        let m = self.bytes[self.pos + 1];
        if ff != 0xFF {
            return Err(super::marker::JpegError::BadMarkerPrefix(ff));
        }
        self.pos += 2;
        Ok(m)
    }

    /// Читает 1 бит. Возвращает 0 или 1.
    pub fn read_bit(&mut self) -> Result<u8, JpegError> {
        if self.buffer_bits == 0 {
            self.refill(1)?;
        }
        self.buffer_bits -= 1;
        Ok(((self.buffer >> self.buffer_bits) & 1) as u8)
    }

    /// Читает n битов (0..=16). Возвращает значение в младших битах.
    pub fn read_bits(&mut self, n: u8) -> Result<u16, JpegError> {
        if n == 0 {
            return Ok(0);
        }
        debug_assert!(n <= 16, "read_bits ограничен 16 битами");
        if self.buffer_bits < n {
            self.refill(n)?;
        }
        self.buffer_bits -= n;
        let mask = if n == 32 {
            u32::MAX
        } else {
            (1u32 << n) - 1
        };
        Ok(((self.buffer >> self.buffer_bits) & mask) as u16)
    }

    /// Подкачивает буфер до `want` битов. На marker-байт прекращает; если
    /// после остановки `buffer_bits < want` — возвращает `UnexpectedEndOfBitstream`.
    fn refill(&mut self, want: u8) -> Result<(), JpegError> {
        while self.buffer_bits < want {
            if self.marker.is_some() {
                return Err(JpegError::UnexpectedEndOfBitstream);
            }
            if self.pos >= self.bytes.len() {
                return Err(JpegError::UnexpectedEndOfBitstream);
            }
            let b = self.bytes[self.pos];
            self.pos += 1;
            if b == 0xFF {
                // По §F.1.2.3 после 0xFF в scan-data всегда должен идти ещё байт.
                if self.pos >= self.bytes.len() {
                    return Err(JpegError::UnexpectedEndOfBitstream);
                }
                let nxt = self.bytes[self.pos];
                self.pos += 1;
                if nxt == 0x00 {
                    // Stuffed FF — это data-байт.
                    self.buffer = (self.buffer << 8) | 0xFF;
                    self.buffer_bits += 8;
                } else {
                    // Реальный маркер. Запоминаем и прекращаем refill.
                    self.marker = Some(nxt);
                    if self.buffer_bits < want {
                        return Err(JpegError::UnexpectedEndOfBitstream);
                    }
                    break;
                }
            } else {
                self.buffer = (self.buffer << 8) | u32::from(b);
                self.buffer_bits += 8;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bits_basic() {
        let bytes = [0b1010_1100u8, 0b0011_0101];
        let mut r = JpegBitReader::new(&bytes, 0);
        assert_eq!(r.read_bits(4).unwrap(), 0b1010);
        assert_eq!(r.read_bits(4).unwrap(), 0b1100);
        assert_eq!(r.read_bits(8).unwrap(), 0b0011_0101);
    }

    #[test]
    fn read_bit_msb_first() {
        let bytes = [0b1000_0000u8];
        let mut r = JpegBitReader::new(&bytes, 0);
        assert_eq!(r.read_bit().unwrap(), 1);
        assert_eq!(r.read_bit().unwrap(), 0);
    }

    #[test]
    fn read_zero_bits_no_advance() {
        let bytes = [0xFFu8, 0x00];
        let mut r = JpegBitReader::new(&bytes, 0);
        // read_bits(0) ничего не должен изменить.
        assert_eq!(r.read_bits(0).unwrap(), 0);
        assert_eq!(r.position(), 0);
    }

    #[test]
    fn read_across_byte_boundary() {
        let bytes = [0b1111_0000u8, 0b1100_0011];
        let mut r = JpegBitReader::new(&bytes, 0);
        // 12 битов: 1111_0000_1100.
        assert_eq!(r.read_bits(12).unwrap(), 0b1111_0000_1100);
    }

    #[test]
    fn byte_stuffing_ff00_becomes_ff() {
        // FF 00 → 0xFF как data.
        let bytes = [0xFFu8, 0x00, 0x55];
        let mut r = JpegBitReader::new(&bytes, 0);
        assert_eq!(r.read_bits(8).unwrap(), 0xFF);
        assert_eq!(r.read_bits(8).unwrap(), 0x55);
    }

    #[test]
    fn marker_stops_refill_and_is_peekable() {
        // 0x55 + FF D9 (EOI). После 0x55 — marker, дальше читать нельзя.
        let bytes = [0x55u8, 0xFF, 0xD9, 0x00];
        let mut r = JpegBitReader::new(&bytes, 0);
        assert_eq!(r.read_bits(8).unwrap(), 0x55);
        // Следующий read должен дать ошибку, поскольку буфер пуст и впереди маркер.
        assert_eq!(r.read_bit().unwrap_err(), JpegError::UnexpectedEndOfBitstream);
        // Маркер виден через peek.
        assert_eq!(r.peek_marker(), Some(0xD9));
    }

    #[test]
    fn consume_marker_aligns_and_clears() {
        let bytes = [0xFFu8, 0xD0, 0xAA];
        let mut r = JpegBitReader::new(&bytes, 0);
        // Прочитаем 1 бит, refill встретит маркер FF D0.
        let res = r.read_bit();
        assert!(res.is_err());
        assert_eq!(r.peek_marker(), Some(0xD0));
        r.consume_marker();
        assert_eq!(r.peek_marker(), None);
        // pos уже за маркером — дальше 0xAA.
        assert_eq!(r.read_bits(8).unwrap(), 0xAA);
    }

    #[test]
    fn truncated_stream_errors() {
        let bytes: [u8; 0] = [];
        let mut r = JpegBitReader::new(&bytes, 0);
        assert_eq!(r.read_bit().unwrap_err(), JpegError::UnexpectedEndOfBitstream);
    }

    #[test]
    fn ff_at_eof_without_continuation_errors() {
        // Одиночный 0xFF в конце — невалидно (после него должен быть data 0x00 или marker).
        let bytes = [0xFFu8];
        let mut r = JpegBitReader::new(&bytes, 0);
        assert_eq!(r.read_bit().unwrap_err(), JpegError::UnexpectedEndOfBitstream);
    }
}
