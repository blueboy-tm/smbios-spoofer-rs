//! Result of one named-device attachment attempt. The NTSTATUS belongs to
//! the named call, including when it returned success with a null output.
#[derive(Clone, Copy)]
pub enum Outcome {
    Capacity,
    Lookup(i32),
    Create(i32),
    Attach(i32),
    Attached(i32),
}

#[cfg(any(feature = "attachment-diagnostics", test))]
impl Outcome {
    pub fn word(self) -> u64 {
        let (stage, status) = match self {
            Self::Capacity => (1, 0),
            Self::Lookup(s) => (2, s),
            Self::Create(s) => (3, s),
            Self::Attach(s) => (4, s),
            Self::Attached(s) => (5, s),
        };
        (stage << 32) | u64::from(status as u32)
    }
}

#[cfg(any(feature = "attachment-diagnostics", test))]
#[repr(C)]
#[derive(Default)]
pub struct Record {
    time: core::sync::atomic::AtomicU64,
    outcome: core::sync::atomic::AtomicU64,
}

#[cfg(any(feature = "attachment-diagnostics", test))]
impl Record {
    pub const fn empty() -> Self {
        use core::sync::atomic::AtomicU64;
        Self {
            time: AtomicU64::new(0),
            outcome: AtomicU64::new(0),
        }
    }

    pub fn publish(&self, time: u64, outcome: Outcome) {
        use core::sync::atomic::Ordering;
        self.time.store(time, Ordering::Relaxed);
        self.outcome.store(outcome.word(), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    #[test]
    fn debugger_layout_and_stage_status_are_stable() {
        assert_eq!(core::mem::size_of::<Record>(), 16);
        assert_eq!(core::mem::offset_of!(Record, outcome), 8);
        let record = Record::empty();
        assert_eq!(record.outcome.load(Ordering::Acquire), 0);
        for (outcome, expected) in [
            (Outcome::Capacity, 0x1_00000000),
            (Outcome::Lookup(-1), 0x2_ffffffff),
            (Outcome::Create(-2), 0x3_fffffffe),
            (Outcome::Attach(0), 0x4_00000000),
            (Outcome::Attached(0), 0x5_00000000),
        ] {
            record.publish(123, outcome);
            assert_eq!(record.outcome.load(Ordering::Acquire), expected);
            assert_eq!(record.time.load(Ordering::Relaxed), 123);
        }
    }
}
