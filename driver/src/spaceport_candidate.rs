//! Read-only, bounded test of the unverified Spaceport serial-offset hypothesis.
//! No candidate text or bytes are exported; this is not a writable span.
use core::sync::atomic::{AtomicU64, Ordering};

const OFFSET_FIELD: usize = 0xA3C;
const MAX_UNITS: usize = 256;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Plausible = 1,
    Unsuccessful = 2,
    BadLength = 3,
    TruncatedField = 4,
    InvalidOffset = 5,
    NoTerminator = 6,
    InvalidUtf16 = 7,
    NullBuffer = 8,
}

impl Check {
    /// The candidate is a LE u32 byte offset relative to output start.
    /// This merely checks shape; it does not prove what the consumer reads.
    pub fn inspect(output: &[u8]) -> (Self, usize, usize) {
        let Some(field) = output.get(OFFSET_FIELD..OFFSET_FIELD + 4) else {
            return (Self::TruncatedField, 0, 0);
        };
        let offset = u32::from_le_bytes([field[0], field[1], field[2], field[3]]) as usize;
        if offset < OFFSET_FIELD + 4 || offset % 2 != 0 || offset > output.len().saturating_sub(2) {
            return (Self::InvalidOffset, offset, 0);
        }

        let mut end = offset;
        let mut units = 0usize;
        let mut leading_high_surrogate = false;
        let mut terminated = false;
        while end + 1 < output.len() && units < MAX_UNITS {
            let unit = u16::from_le_bytes([output[end], output[end + 1]]);
            end += 2;
            if unit == 0 {
                terminated = true;
                break;
            }
            units += 1;
            if (0xD800..=0xDBFF).contains(&unit) {
                if leading_high_surrogate { return (Self::InvalidUtf16, offset, units); }
                leading_high_surrogate = true;
            } else if (0xDC00..=0xDFFF).contains(&unit) {
                if !leading_high_surrogate { return (Self::InvalidUtf16, offset, units); }
                leading_high_surrogate = false;
            } else if leading_high_surrogate {
                return (Self::InvalidUtf16, offset, units);
            }
        }
        if !terminated { return (Self::NoTerminator, offset, units); }
        if leading_high_surrogate { return (Self::InvalidUtf16, offset, units); }
        (Self::Plausible, offset, units)
    }
}

/// Five QWORDs; stable debugger layout. Values contain no identifier bytes.
#[repr(C)]
pub struct CandidateSnapshot {
    pub checks: AtomicU64,       // +0x00
    pub plausible: AtomicU64,    // +0x08
    pub last_result: AtomicU64,  // +0x10: Check discriminant
    pub last_offset: AtomicU64,  // +0x18: untrusted candidate offset
    pub last_units: AtomicU64,   // +0x20: non-NUL UTF-16 units inspected
}

impl CandidateSnapshot {
    pub const fn empty() -> Self {
        Self {
            checks: AtomicU64::new(0),
            plausible: AtomicU64::new(0),
            last_result: AtomicU64::new(0),
            last_offset: AtomicU64::new(0),
            last_units: AtomicU64::new(0),
        }
    }
    pub fn record(&self, result: (Check, usize, usize)) {
        let (check, offset, units) = result;
        self.last_offset.store(offset as u64, Ordering::Relaxed);
        self.last_units.store(units as u64, Ordering::Relaxed);
        self.last_result.store(check as u64, Ordering::Relaxed);
        if check == Check::Plausible {
            self.plausible.fetch_add(1, Ordering::Relaxed);
        }
        self.checks.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(offset: usize, value: &[u16]) -> Vec<u8> {
        let mut out = vec![0u8; 0xDA2];
        out[OFFSET_FIELD..OFFSET_FIELD + 4].copy_from_slice(&(offset as u32).to_le_bytes());
        for (i, &unit) in value.iter().enumerate() {
            let begin = offset + i * 2;
            out[begin..begin + 2].copy_from_slice(&unit.to_le_bytes());
        }
        out
    }
    #[test]
    fn accepts_bounded_nul_terminated_candidate() {
        let s = response(0xA60, &[b'A' as u16, b'1' as u16, 0]);
        assert_eq!(Check::inspect(&s), (Check::Plausible, 0xA60, 2));
    }
    #[test]
    fn rejects_short_and_untrusted_offsets() {
        assert_eq!(Check::inspect(&[0; 8]).0, Check::TruncatedField);
        let s = response(0xA3C, &[0]);
        assert_eq!(Check::inspect(&s).0, Check::InvalidOffset);
        let mut s = response(0xA60, &[0]);
        s[OFFSET_FIELD..OFFSET_FIELD + 4].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        assert_eq!(Check::inspect(&s).0, Check::InvalidOffset);
    }
    #[test]
    fn rejects_unterminated_and_invalid_utf16() {
        let s = response(0xDA0, &[0x41]);
        assert_eq!(Check::inspect(&s).0, Check::NoTerminator);
        let s = response(0xA60, &[0xDC00, 0]);
        assert_eq!(Check::inspect(&s).0, Check::InvalidUtf16);
        let s = response(0xA60, &[0xD800, 0]);
        assert_eq!(Check::inspect(&s).0, Check::InvalidUtf16);
        let s = response(0xA60, &[0xD83D, 0xDE00, 0]);
        assert_eq!(Check::inspect(&s).0, Check::Plausible);
    }
    #[test]
    fn snapshot_is_five_qwords() {
        assert_eq!(core::mem::size_of::<CandidateSnapshot>(), 40);
        let snapshot = CandidateSnapshot::empty();
        snapshot.record((Check::Plausible, 0xA60, 2));
        assert_eq!(snapshot.checks.load(Ordering::Acquire), 1);
        assert_eq!(snapshot.plausible.load(Ordering::Relaxed), 1);
        assert_eq!(snapshot.last_offset.load(Ordering::Relaxed), 0xA60);
    }
}
