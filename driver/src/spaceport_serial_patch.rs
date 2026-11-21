//! Experimental, opt-in serial-only rewrite of one observed Spaceport response.
//! This is NOT a generic Spaceport format implementation. No EUI/UniqueId fields
//! or pointers are changed. An exact old-string guard prevents blind writes.
use core::sync::atomic::{AtomicU64, Ordering};

const OFFSET_FIELD: usize = 0xA3C;
const EXPECTED_OFFSET: usize = 0xBE4;
const OLD_SERIAL: &[u8; 40] = b"0000_0000_0000_0001_00A0_7523_453E_0948.";
const OLD_UNITS: usize = OLD_SERIAL.len();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Rewrite {
    Patched = 1,
    BadResponse = 2,
    WrongOffset = 3,
    WrongOldSerial = 4,
    InvalidReplacement = 5,
}

#[repr(C)]
pub struct PatchSnapshot {
    pub attempts: AtomicU64,
    pub patched: AtomicU64,
    pub last_result: AtomicU64,
}

impl PatchSnapshot {
    pub const fn empty() -> Self {
        Self {
            attempts: AtomicU64::new(0),
            patched: AtomicU64::new(0),
            last_result: AtomicU64::new(0),
        }
    }

    pub fn record(&self, result: Rewrite) {
        self.last_result.store(result as u64, Ordering::Relaxed);
        if result == Rewrite::Patched {
            self.patched.fetch_add(1, Ordering::Relaxed);
        }
        self.attempts.fetch_add(1, Ordering::Release);
    }
}

/// Writes only within the existing, verified UTF-16 string (40 units plus NUL).
/// The replacement comes from the validated v9 profile's 20-byte serial.
/// This function cannot guarantee that the consuming provider uses an
/// independent UniqueId field: that must be checked on the running system.
pub fn rewrite(output: &mut [u8], serial: &[u8; 20]) -> Rewrite {
    let Some(field) = output.get(OFFSET_FIELD..OFFSET_FIELD + 4) else {
        return Rewrite::BadResponse;
    };
    let offset = u32::from_le_bytes([field[0], field[1], field[2], field[3]]) as usize;
    if offset != EXPECTED_OFFSET {
        return Rewrite::WrongOffset;
    }

    let Some(region) = output.get_mut(offset..offset + (OLD_UNITS + 1) * 2) else {
        return Rewrite::BadResponse;
    };

    // Exact compare of all 40 UTF-16LE code units and the terminator.
    // No mutation occurs until all guards pass.
    for (i, &byte) in OLD_SERIAL.iter().enumerate() {
        if region[i * 2] != byte || region[i * 2 + 1] != 0 {
            return Rewrite::WrongOldSerial;
        }
    }
    if region[OLD_UNITS * 2] != 0 || region[OLD_UNITS * 2 + 1] != 0 {
        return Rewrite::WrongOldSerial;
    }

    // A v9 profile uses printable ASCII padded with spaces. Require at
    // least four alphanumeric bytes and reject non-space text after padding.
    let mut length = 0usize;
    let mut non_space = 0usize;
    let mut padding_started = false;
    for &byte in serial {
        if byte == b' ' {
            padding_started = true;
        } else {
            if padding_started || !byte.is_ascii_alphanumeric() {
                return Rewrite::InvalidReplacement;
            }
            non_space += 1;
            length += 1;
        }
    }
    if non_space < 4 {
        return Rewrite::InvalidReplacement;
    }

    // Keep the allocated response structure/length/offset unchanged.
    // Zero-fill ONLY the old string's UTF-16 storage and append NUL.
    for b in region.iter_mut() { *b = 0; }
    for (i, &byte) in serial[..length].iter().enumerate() {
        region[i * 2] = byte;
    }
    Rewrite::Patched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut output = vec![0xCC; 0xDA2];
        output[OFFSET_FIELD..OFFSET_FIELD + 4]
            .copy_from_slice(&(EXPECTED_OFFSET as u32).to_le_bytes());
        for (i, &byte) in OLD_SERIAL.iter().enumerate() {
            output[EXPECTED_OFFSET + i * 2] = byte;
            output[EXPECTED_OFFSET + i * 2 + 1] = 0;
        }
        output[EXPECTED_OFFSET + OLD_UNITS * 2] = 0;
        output[EXPECTED_OFFSET + OLD_UNITS * 2 + 1] = 0;
        output
    }

    #[test]
    fn rewrites_only_original_string_storage() {
        let mut output = fixture();
        let before = output.clone();
        let mut replacement = [b' '; 20];
        replacement[..16].copy_from_slice(b"D1F9V96CQUN4DJ7X");
        assert_eq!(rewrite(&mut output, &replacement), Rewrite::Patched);
        assert_eq!(&output[..EXPECTED_OFFSET], &before[..EXPECTED_OFFSET]);
        assert_eq!(
            &output[EXPECTED_OFFSET + 82..],
            &before[EXPECTED_OFFSET + 82..]
        );
        for (i, &byte) in replacement[..16].iter().enumerate() {
            assert_eq!(output[EXPECTED_OFFSET + i * 2], byte);
            assert_eq!(output[EXPECTED_OFFSET + i * 2 + 1], 0);
        }
        assert!(output[EXPECTED_OFFSET + 32..EXPECTED_OFFSET + 82].iter().all(|&b| b == 0));
    }

    #[test]
    fn wrong_serial_and_offset_never_write() {
        let mut output = fixture();
        output[EXPECTED_OFFSET] = b'Z';
        let original = output.clone();
        let replacement = *b"D1F9V96CQUN4DJ7X    ";
        assert_eq!(rewrite(&mut output, &replacement), Rewrite::WrongOldSerial);
        assert_eq!(output, original);

        let mut output = fixture();
        output[OFFSET_FIELD] = 0;
        let original = output.clone();
        assert_eq!(rewrite(&mut output, &replacement), Rewrite::WrongOffset);
        assert_eq!(output, original);
    }

    #[test]
    fn rejects_short_and_bad_profile_without_write() {
        let mut output = fixture();
        let original = output.clone();
        assert_eq!(rewrite(&mut output[..0xDA0], b"D1F9V96CQUN4DJ7X    "), Rewrite::BadResponse);
        assert_eq!(output, original);
        assert_eq!(rewrite(&mut output, &[b' '; 20]), Rewrite::InvalidReplacement);
        assert_eq!(output, original);
    }

    #[test]
    fn snapshot_layout() {
        assert_eq!(core::mem::size_of::<PatchSnapshot>(), 24);
        let s = PatchSnapshot::empty();
        s.record(Rewrite::Patched);
        assert_eq!(s.patched.load(Ordering::Relaxed), 1);
        assert_eq!(s.attempts.load(Ordering::Acquire), 1);
    }
}
