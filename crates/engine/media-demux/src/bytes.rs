//! Мелкие помощники разбора: big-endian читатель и обход ISOBMFF-боксов.
//!
//! Ничего не паникует на обрезанных или враждебных данных: любое чтение за
//! пределами среза даёт `None`, а обход боксов останавливается на первом же
//! повреждённом заголовке.

/// Последовательный big-endian читатель по срезу.
pub(crate) struct Be<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Be<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn skip(&mut self, n: usize) -> Option<()> {
        let end = self.pos.checked_add(n)?;
        (end <= self.data.len()).then(|| self.pos = end)
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|b| b[0])
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.bytes(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.bytes(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        self.bytes(8).map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_be_bytes(a)
        })
    }
}

/// Один бокс ISOBMFF: тип, абсолютное смещение начала и содержимое.
#[derive(Clone, Copy)]
pub(crate) struct BoxRef<'a> {
    pub kind: [u8; 4],
    /// Смещение первого байта заголовка бокса от начала файла.
    pub start: usize,
    pub payload: &'a [u8],
    /// Смещение первого байта содержимого от начала файла.
    pub payload_start: usize,
}

/// Дочерние боксы `data`, лежащей в файле по смещению `base`. Размер 0
/// («до конца») и 1 («64-битный размер») поддержаны; обход прекращается на
/// первом боксе, чей размер не укладывается в срез.
pub(crate) fn children(data: &[u8], base: usize) -> Vec<BoxRef<'_>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while data.len().saturating_sub(pos) >= 8 {
        let mut r = Be::new(&data[pos..]);
        let (Some(size32), Some(kind)) = (r.u32(), r.bytes(4)) else {
            break;
        };
        let kind = [kind[0], kind[1], kind[2], kind[3]];
        let (header, total) = match size32 {
            0 => (8, data.len() - pos),
            1 => match r.u64().and_then(|s| usize::try_from(s).ok()) {
                Some(s) => (16, s),
                None => break,
            },
            n => (8, n as usize),
        };
        if total < header || total > data.len() - pos {
            break;
        }
        out.push(BoxRef {
            kind,
            start: base + pos,
            payload: &data[pos + header..pos + total],
            payload_start: base + pos + header,
        });
        pos += total;
    }
    out
}

/// Первый дочерний бокс заданного типа.
pub(crate) fn find<'a>(data: &'a [u8], base: usize, kind: &[u8; 4]) -> Option<BoxRef<'a>> {
    children(data, base).into_iter().find(|b| &b.kind == kind)
}

/// Спуск по цепочке типов (`moov/trak/mdia`…).
pub(crate) fn find_path<'a>(
    data: &'a [u8],
    base: usize,
    path: &[&[u8; 4]],
) -> Option<BoxRef<'a>> {
    let (first, rest) = path.split_first()?;
    let mut cur = find(data, base, first)?;
    for k in rest {
        cur = find(cur.payload, cur.payload_start, k)?;
    }
    Some(cur)
}
