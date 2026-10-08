//! Быстрый хешер для ключей-идентификаторов (`u32`-индексы и упакованные `NodeId`).
//!
//! `std::collections::HashSet<u32>` хеширует SipHash-1-3, рассчитанным на защиту от
//! hash-flooding. Ключи, которыми пользуется движок (индекс узла в арене, `NodeId::raw`),
//! выдаёт сам движок: подобрать их снаружи нельзя, а плотные последовательные числа SipHash
//! только тормозит — на каждый флаш с `body` в корне набор ключей строится и опрашивается
//! по числу боксов документа (BUG-935 срез 77).
//!
//! Тип подходит только для таких ключей. Строки и всё, что приходит из страницы, остаются
//! на стандартном хешере.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// Множество с ключами-идентификаторами и [`IdHasher`].
pub type IdSet<K> = HashSet<K, BuildHasherDefault<IdHasher>>;

/// Отображение с ключами-идентификаторами и [`IdHasher`].
pub type IdMap<K, V> = HashMap<K, V, BuildHasherDefault<IdHasher>>;

/// Множитель Фибоначчи: `2^64 / φ`, нечётный. Умножение размазывает близкие числа по
/// старшим битам, которые `hashbrown` берёт для байта-метки.
const MUL: u64 = 0x9E37_79B9_7F4A_7C15;

/// Хешер одного целого: умножение на константу Фибоначчи со сворачиванием старшей половины.
#[derive(Default, Clone, Copy)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 32)
    }

    fn write(&mut self, bytes: &[u8]) {
        // Ключ из нескольких полей или байтов: свернуть побайтно, чтобы тип остался
        // корректным хешером, хотя горячий путь — `write_u32`/`write_u64`.
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(MUL);
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.write_u64(u64::from(i));
    }

    fn write_u32(&mut self, i: u32) {
        self.write_u64(u64::from(i));
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(MUL);
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_map_behave_like_the_std_ones() {
        let mut set: IdSet<u32> = IdSet::default();
        let mut map: IdMap<u32, u32> = IdMap::default();
        for i in 0..10_000u32 {
            assert!(set.insert(i * 3));
            map.insert(i * 3, i);
        }
        assert!(!set.insert(30));
        assert_eq!(set.len(), 10_000);
        assert!(set.contains(&2_997));
        assert!(!set.contains(&2_998));
        assert_eq!(map.get(&9_999), Some(&3_333));
        assert_eq!(map.remove(&9_999), Some(3_333));
        assert_eq!(map.get(&9_999), None);
    }

    #[test]
    fn dense_sequential_keys_spread_over_the_high_byte() {
        // `hashbrown` reads the top 7 bits as the control byte: dense ids must not all share it.
        let mut tops = std::collections::HashSet::new();
        for i in 0..4_096u32 {
            let mut h = IdHasher::default();
            h.write_u32(i);
            tops.insert(h.finish() >> 57);
        }
        assert!(tops.len() > 100, "only {} distinct control bytes", tops.len());
    }

    #[test]
    fn byte_slices_hash_without_collapsing() {
        let mut a = IdHasher::default();
        a.write(b"ab");
        let mut b = IdHasher::default();
        b.write(b"ba");
        assert_ne!(a.finish(), b.finish());
    }
}
