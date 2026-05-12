//! Big-endian бинарный reader для разбора TrueType / OpenType структур.
//!
//! Все методы возвращают `Option`: `None` означает «не хватило байт» — это
//! битый или обрезанный шрифт, должен корректно ловиться вышестоящим
//! парсером, а не паниковать.

#[derive(Debug, Clone)]
pub struct BinaryReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BinaryReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn skip(&mut self, n: usize) -> Option<()> {
        let new_pos = self.pos.checked_add(n)?;
        if new_pos > self.data.len() {
            return None;
        }
        self.pos = new_pos;
        Some(())
    }

    pub fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    pub fn read_u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    pub fn read_u16(&mut self) -> Option<u16> {
        let bytes: [u8; 2] = self.read_bytes(2)?.try_into().ok()?;
        Some(u16::from_be_bytes(bytes))
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        let bytes: [u8; 4] = self.read_bytes(4)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    }

    pub fn read_i16(&mut self) -> Option<i16> {
        let bytes: [u8; 2] = self.read_bytes(2)?.try_into().ok()?;
        Some(i16::from_be_bytes(bytes))
    }

    pub fn read_i32(&mut self) -> Option<i32> {
        let bytes: [u8; 4] = self.read_bytes(4)?.try_into().ok()?;
        Some(i32::from_be_bytes(bytes))
    }

    /// 4-байтовый ASCII-тег (например, `b"head"`, `b"glyf"`).
    pub fn read_tag(&mut self) -> Option<[u8; 4]> {
        let bytes: [u8; 4] = self.read_bytes(4)?.try_into().ok()?;
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_sequential_integers() {
        let data = [0x00, 0x01, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
        let mut r = BinaryReader::new(&data);
        assert_eq!(r.read_u8(), Some(0x00));
        assert_eq!(r.read_u8(), Some(0x01));
        assert_eq!(r.read_u16(), Some(0x1234));
        assert_eq!(r.read_u32(), Some(0x56789abc));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn signed_negative() {
        let data = [0xff, 0xff, 0xff, 0xff, 0xff, 0xfe];
        let mut r = BinaryReader::new(&data);
        assert_eq!(r.read_i16(), Some(-1));
        assert_eq!(r.read_i32(), Some(-2));
    }

    #[test]
    fn tag_is_four_ascii_bytes() {
        let data = b"headglyf";
        let mut r = BinaryReader::new(data);
        assert_eq!(r.read_tag(), Some(*b"head"));
        assert_eq!(r.read_tag(), Some(*b"glyf"));
    }

    #[test]
    fn read_past_end_returns_none() {
        let data = [0x01, 0x02];
        let mut r = BinaryReader::new(&data);
        assert_eq!(r.read_u32(), None); // need 4, have 2
        assert_eq!(r.position(), 0); // не двигаемся при неудаче
    }

    #[test]
    fn seek_and_skip() {
        let data = [0xa, 0xb, 0xc, 0xd, 0xe];
        let mut r = BinaryReader::new(&data);
        r.skip(3).unwrap();
        assert_eq!(r.read_u8(), Some(0xd));
        r.seek(1);
        assert_eq!(r.read_u8(), Some(0xb));
    }

    #[test]
    fn skip_past_end_returns_none() {
        let data = [0x01, 0x02];
        let mut r = BinaryReader::new(&data);
        assert_eq!(r.skip(10), None);
    }
}
