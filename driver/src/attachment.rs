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

/// Return true only for an existing Spaceport detailed-drive request with
/// nonzero caller output capacity. No buffer contents are inspected.
#[cfg(any(feature = "attachment-diagnostics", test))]
pub fn should_count_spaceport(is_spaceport: bool, ioctl: u32, output_capacity: usize) -> bool {
    is_spaceport && ioctl == 0x00E7_0408 && output_capacity != 0
}

/// Debugger-readable counters for the *existing* Spaceport detailed-drive IOCTL.
/// These counters never inspect an input or output buffer.
#[cfg(any(feature = "attachment-diagnostics", test))]
#[repr(C)]
pub struct SpaceportIoctlCounters {
    pub requests: core::sync::atomic::AtomicU64,
    pub completions: core::sync::atomic::AtomicU64,
    pub successful_completions: core::sync::atomic::AtomicU64,
    pub last_status: core::sync::atomic::AtomicU64,
    pub last_information: core::sync::atomic::AtomicU64,
}

#[cfg(any(feature = "attachment-diagnostics", test))]
impl SpaceportIoctlCounters {
    pub const fn empty() -> Self {
        use core::sync::atomic::AtomicU64;
        Self {
            requests: AtomicU64::new(0),
            completions: AtomicU64::new(0),
            successful_completions: AtomicU64::new(0),
            last_status: AtomicU64::new(0),
            last_information: AtomicU64::new(0),
        }
    }

    pub fn on_request(&self) {
        self.requests.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    pub fn on_completion(&self, status: i32, information: usize) {
        use core::sync::atomic::Ordering;
        self.last_status.store(u64::from(status as u32), Ordering::Relaxed);
        self.last_information.store(information as u64, Ordering::Relaxed);
        if status >= 0 {
            self.successful_completions.fetch_add(1, Ordering::Relaxed);
        }
        self.completions.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    #[test]
    fn only_expected_spaceport_requests_are_counted() {
        assert!(should_count_spaceport(true, 0x00E7_0408, 8192));
        assert!(!should_count_spaceport(false, 0x00E7_0408, 8192));
        assert!(!should_count_spaceport(true, 0x00E7_0404, 8192));
        assert!(!should_count_spaceport(true, 0x00E7_0408, 0));
    }

    #[test]
    fn spaceport_counters_have_fixed_debugger_layout() {
        assert_eq!(core::mem::size_of::<SpaceportIoctlCounters>(), 40);
        assert_eq!(core::mem::offset_of!(SpaceportIoctlCounters, requests), 0);
        assert_eq!(core::mem::offset_of!(SpaceportIoctlCounters, completions), 8);
        assert_eq!(core::mem::offset_of!(SpaceportIoctlCounters, successful_completions), 16);
        assert_eq!(core::mem::offset_of!(SpaceportIoctlCounters, last_status), 24);
        assert_eq!(core::mem::offset_of!(SpaceportIoctlCounters, last_information), 32);
        let counters = SpaceportIoctlCounters::empty();
        counters.on_request();
        counters.on_request();
        counters.on_completion(0, 0x1234);
        counters.on_completion(-1073741823, 0);
        assert_eq!(counters.requests.load(Ordering::Relaxed), 2);
        assert_eq!(counters.completions.load(Ordering::Acquire), 2);
        assert_eq!(counters.successful_completions.load(Ordering::Relaxed), 1);
        assert_eq!(counters.last_status.load(Ordering::Relaxed), 0xC0000001);
        assert_eq!(counters.last_information.load(Ordering::Relaxed), 0);
    }

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
