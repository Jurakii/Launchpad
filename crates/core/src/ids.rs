//! Item ids. They become file and folder names (`icons/<id>.png`,
//! `files/<id>/`), which is why `is_uuid` is strict.
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

/// A random version 4 UUID, like `crypto.randomUUID()`.
pub fn new_uuid() -> String {
    // shortcut: std's per-process hash seeds as the random source, fine for ids
    // (not secrets). Move to BCryptGenRandom if an id ever guards anything.
    let word = || RandomState::new().build_hasher().finish();
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&word().to_le_bytes());
    b[8..].copy_from_slice(&word().to_le_bytes());
    b[6] = b[6] & 0x0f | 0x40;
    b[8] = b[8] & 0x3f | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// 8-4-4-4-12 hex digits, any case, nothing else.
pub fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_valid_and_differ() {
        let a = new_uuid();
        assert!(is_uuid(&a), "{a}");
        assert_eq!(a.as_bytes()[14], b'4');
        assert_ne!(a, new_uuid());
    }

    #[test]
    fn is_uuid_refuses_path_parts() {
        assert!(!is_uuid("../../etc"));
        assert!(!is_uuid("12345678-1234-1234-1234-1234567890a/"));
        assert!(is_uuid("12345678-1234-4234-8234-1234567890AB"));
    }
}
