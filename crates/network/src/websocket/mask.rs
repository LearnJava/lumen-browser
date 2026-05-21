/// RFC 6455 §5.3 — XOR masking/unmasking. Mask and unmask are the same operation.
/// Client-to-server frames MUST be masked; server-to-client frames MUST NOT.
pub(crate) fn apply(payload: &mut [u8], key: [u8; 4]) {
    for (i, byte) in payload.iter_mut().enumerate() {
        *byte ^= key[i & 3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_roundtrip() {
        let key = [0x37, 0xfa, 0x21, 0x3d];
        let original = b"Hello, world!";
        let mut buf = original.to_vec();
        apply(&mut buf, key);
        assert_ne!(buf, original);
        apply(&mut buf, key);
        assert_eq!(buf, original);
    }

    #[test]
    fn apply_empty() {
        let mut buf: Vec<u8> = Vec::new();
        apply(&mut buf, [0xFF; 4]);
        assert!(buf.is_empty());
    }

    #[test]
    fn key_cycles_every_four_bytes() {
        let key = [1, 2, 3, 4];
        let mut buf = vec![0u8; 8];
        apply(&mut buf, key);
        assert_eq!(&buf[..4], &buf[4..]);
    }
}
