//! This module is an implemention of a namespacing scheme described
//! in https://github.com/webmaster128/key-namespacing#length-prefixed-keys
//!
//! Everything in this file is only responsible for building such keys
//! and is in no way specific to any kind of storage.

/// Calculates the raw key prefix for a given namespace as documented
/// in https://github.com/webmaster128/key-namespacing#length-prefixed-keys
pub(crate) fn key_prefix(namespace: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(namespace.len() + 2);
    out.extend_from_slice(&key_len(namespace));
    out.extend_from_slice(namespace);
    out
}

/// Calculates the raw key prefix for a given nested namespace
/// as documented in https://github.com/webmaster128/key-namespacing#nesting
pub(crate) fn key_prefix_nested(namespaces: &[&[u8]]) -> Vec<u8> {
    let mut size = namespaces.len();
    for &namespace in namespaces {
        size += namespace.len() + 2;
    }

    let mut out = Vec::with_capacity(size);
    for &namespace in namespaces {
        out.extend_from_slice(&key_len(namespace));
        out.extend_from_slice(namespace);
    }
    out
}

// returns the length as a 2 byte big endian encoded integer
fn key_len(prefix: &[u8]) -> [u8; 2] {
    if prefix.len() > 0xFFFF {
        panic!("only supports namespaces up to length 0xFFFF")
    }
    let length_bytes = (prefix.len() as u64).to_be_bytes();
    [length_bytes[6], length_bytes[7]]
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn key_prefix_works() {
        assert_eq!(key_prefix(b""), b"\x00\x00");
        assert_eq!(key_prefix(b"a"), b"\x00\x01a");
        assert_eq!(key_prefix(b"ab"), b"\x00\x02ab");
        assert_eq!(key_prefix(b"abc"), b"\x00\x03abc");
    }

    #[test]
    fn key_prefix_works_for_long_prefix() {
        let long_namespace1 = vec![0; 256];
        let prefix1 = key_prefix(&long_namespace1);
        assert_eq!(prefix1.len(), 256 + 2);
        assert_eq!(&prefix1[0..2], b"\x01\x00");

        let long_namespace2 = vec![0; 30000];
        let prefix2 = key_prefix(&long_namespace2);
        assert_eq!(prefix2.len(), 30000 + 2);
        assert_eq!(&prefix2[0..2], b"\x75\x30");

        let long_namespace3 = vec![0; 0xFFFF];
        let prefix3 = key_prefix(&long_namespace3);
        assert_eq!(prefix3.len(), 0xFFFF + 2);
        assert_eq!(&prefix3[0..2], b"\xFF\xFF");
    }

    #[test]
    #[should_panic(expected = "only supports namespaces up to length 0xFFFF")]
    fn key_prefix_panics_for_too_long_prefix() {
        let limit = 0xFFFF;
        let long_namespace = vec![0; limit + 1];
        key_prefix(&long_namespace);
    }

    #[test]
    fn key_prefix_nested_works() {
        assert_eq!(key_prefix_nested(&[]), b"");
        assert_eq!(key_prefix_nested(&[b""]), b"\x00\x00");
        assert_eq!(key_prefix_nested(&[b"", b""]), b"\x00\x00\x00\x00");

        assert_eq!(key_prefix_nested(&[b"a"]), b"\x00\x01a");
        assert_eq!(key_prefix_nested(&[b"a", b"ab"]), b"\x00\x01a\x00\x02ab");
        assert_eq!(
            key_prefix_nested(&[b"a", b"ab", b"abc"]),
            b"\x00\x01a\x00\x02ab\x00\x03abc"
        );
    }
}
