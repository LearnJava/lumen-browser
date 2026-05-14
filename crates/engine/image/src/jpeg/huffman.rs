//! Canonical Huffman таблицы JPEG (ISO/IEC 10918-1 Annex C).
//!
//! После DHT мы знаем массив `bits[1..=16]` — число кодов каждой длины —
//! и список `values` в порядке возрастания длины. Канонические коды
//! строятся последовательно:
//!
//! - первая длина L1 (наименьшая ненулевая): код 0, 1, 2, …, bits[L1-1]-1;
//! - переход к L1+1: код = (последний + 1) << 1;
//! - и так далее.
//!
//! Декодер хранит `min_code[L]`, `max_code[L]` и `value_offset[L]` для каждой
//! длины 1..=16. На каждом бите кода прибавляем bit и сдвигаем; как только
//! текущий accumulated code попадает в `[min_code[L], max_code[L]]` — нашли
//! символ за `value_offset[L] + (code - min_code[L])` в `values`.

use super::bit_reader::JpegBitReader;
use super::marker::JpegError;

#[derive(Debug, Clone)]
pub struct HuffmanTable {
    /// `min_code[L-1]` — наименьший канонический код длины L (или `i32::MAX`,
    /// если ни одного кода такой длины нет).
    min_code: [i32; 16],
    /// `max_code[L-1]` — наибольший канонический код длины L (или `-1`).
    max_code: [i32; 16],
    /// Индекс в `values` первого символа длины L.
    value_offset: [usize; 16],
    /// Все символы в порядке возрастания длины кода.
    values: Vec<u8>,
}

impl HuffmanTable {
    /// Строит таблицу из `bits` (16 счётчиков длин) и `values` (символы
    /// в порядке возрастания длины кода).
    pub fn build(bits: [u8; 16], values: Vec<u8>) -> Result<Self, JpegError> {
        let total: usize = bits.iter().map(|&b| b as usize).sum();
        if total != values.len() {
            return Err(JpegError::BadHuffmanCount(total));
        }
        let mut min_code = [i32::MAX; 16];
        let mut max_code = [-1i32; 16];
        let mut value_offset = [0usize; 16];

        let mut code: i32 = 0;
        let mut value_idx: usize = 0;
        for l in 0..16 {
            let n = bits[l] as i32;
            if n > 0 {
                min_code[l] = code;
                max_code[l] = code + n - 1;
                value_offset[l] = value_idx;
                // Канонические коды длины L+1 не могут превышать 2^(L+1)-1.
                // max_code должен помещаться в (L+1) бит.
                if max_code[l] >= (1i32 << (l + 1)) {
                    return Err(JpegError::BadHuffmanCodes);
                }
                code += n;
                value_idx += n as usize;
            }
            // Переход к следующей длине — добавляем бит.
            code <<= 1;
        }

        Ok(Self {
            min_code,
            max_code,
            value_offset,
            values,
        })
    }

    /// Декодирует один символ из bit-stream-а.
    pub fn decode(&self, reader: &mut JpegBitReader<'_>) -> Result<u8, JpegError> {
        let mut code: i32 = 0;
        for l in 0..16 {
            code = (code << 1) | i32::from(reader.read_bit()?);
            if code <= self.max_code[l] {
                let offset = self.value_offset[l] + (code - self.min_code[l]) as usize;
                if offset >= self.values.len() {
                    return Err(JpegError::BadHuffmanCodes);
                }
                return Ok(self.values[offset]);
            }
        }
        Err(JpegError::BadHuffmanCodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_simple_table() {
        // 2 кода длины 2, 1 код длины 3.
        // Codes: 00, 01 для symbols [10, 20]; 100 для symbol [30].
        let bits = [0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let values = vec![10u8, 20, 30];
        let table = HuffmanTable::build(bits, values).unwrap();

        // Длина 2: min=0, max=1.
        assert_eq!(table.min_code[1], 0);
        assert_eq!(table.max_code[1], 1);
        // Длина 3: min=4 (после двух 2-битных code 0,1; << 1 = 4).
        assert_eq!(table.min_code[2], 4);
        assert_eq!(table.max_code[2], 4);
    }

    #[test]
    fn build_rejects_oversubscribed() {
        // 3 кода длины 1 — невозможно (длина 1 даёт максимум 2 кода: 0, 1).
        let bits = [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let values = vec![1u8, 2, 3];
        assert_eq!(
            HuffmanTable::build(bits, values).unwrap_err(),
            JpegError::BadHuffmanCodes
        );
    }

    #[test]
    fn build_rejects_count_mismatch() {
        let bits = [0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let values = vec![10u8]; // bits.sum() = 2, но values.len() = 1.
        assert_eq!(
            HuffmanTable::build(bits, values).unwrap_err(),
            JpegError::BadHuffmanCount(2)
        );
    }

    #[test]
    fn decode_roundtrip() {
        // Та же таблица: 00→10, 01→20, 100→30.
        let bits = [0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let values = vec![10u8, 20, 30];
        let table = HuffmanTable::build(bits, values).unwrap();

        // Bitstream: 00 01 100 → должно дать 10, 20, 30.
        // Запишем как байт: 0001100_0 (последний бит — padding) = 0x18.
        let bytes = [0x18u8];
        let mut reader = JpegBitReader::new(&bytes, 0);
        assert_eq!(table.decode(&mut reader).unwrap(), 10);
        assert_eq!(table.decode(&mut reader).unwrap(), 20);
        assert_eq!(table.decode(&mut reader).unwrap(), 30);
    }

    #[test]
    fn empty_table_is_valid_but_decodes_to_error() {
        // JPEG разрешает таблицы без кодов (например, для AC, когда все блоки EOB).
        let bits = [0u8; 16];
        let table = HuffmanTable::build(bits, vec![]).unwrap();
        let bytes = [0x00u8];
        let mut reader = JpegBitReader::new(&bytes, 0);
        // Любой decode должен вернуть ошибку — либо все коды длинее доступных
        // битов (EndOfBitstream), либо ни один не подошёл (BadHuffmanCodes).
        assert!(table.decode(&mut reader).is_err());
    }
}
