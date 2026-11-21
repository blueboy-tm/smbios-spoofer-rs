//! Debugger-only, fixed-layout counters for the two storage ID IOCTLs.
//! No device names, serials, raw buffers, or identifiers are stored.
//! These counters are accessible from a kernel debugger, not via user-mode IOCTL.
use core::sync::atomic::{AtomicU64, Ordering};

pub const TAG_ID: u8 = 10;
pub const TAG_DUID: u8 = 9;

/// Exactly nine little-endian u64 fields per property. Stable layout for 'dq'.
#[repr(C)]
pub struct PropertyCounters {
    pub requests: AtomicU64,
    pub completions: AtomicU64,
    pub successful: AtomicU64,
    pub failed: AtomicU64,
    pub invalid_output: AtomicU64,
    pub patch_attempts: AtomicU64,
    pub patched: AtomicU64,
    pub rejected: AtomicU64,
    pub last_status: AtomicU64,
}
impl PropertyCounters {
    pub const fn empty() -> Self {
        const Z: AtomicU64 = AtomicU64::new(0);
        Self {
            requests: Z, completions: AtomicU64::new(0),
            successful: AtomicU64::new(0), failed: AtomicU64::new(0),
            invalid_output: AtomicU64::new(0), patch_attempts: AtomicU64::new(0),
            patched: AtomicU64::new(0), rejected: AtomicU64::new(0),
            last_status: AtomicU64::new(0),
        }
    }
    pub fn on_request(&self) { self.requests.fetch_add(1, Ordering::Relaxed); }
    /// Called once after a registered completion. Valid output is bounded by
    /// both the caller capacity and the bytes returned by the lower driver.
    pub fn on_completion(&self, status: i32, valid: bool) {
        self.last_status.store(status as u32 as u64, Ordering::Relaxed);
        if status < 0 {
            self.failed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.successful.fetch_add(1, Ordering::Relaxed);
            if !valid { self.invalid_output.fetch_add(1, Ordering::Relaxed); }
        }
        self.completions.fetch_add(1, Ordering::Release);
    }
    /// One logical attempt per completion, regardless of SystemBuffer/MDL.
    pub fn on_patch(&self, changed: bool) {
        if changed { self.patched.fetch_add(1, Ordering::Relaxed); }
        else { self.rejected.fetch_add(1, Ordering::Relaxed); }
        self.patch_attempts.fetch_add(1, Ordering::Release);
    }
}

#[repr(C)]
pub struct NamespaceIdCounters {
    pub id: PropertyCounters,
    pub duid: PropertyCounters,
}
impl NamespaceIdCounters {
    pub const fn empty() -> Self {
        Self { id: PropertyCounters::empty(), duid: PropertyCounters::empty() }
    }
    pub fn for_tag(&self, tag: u8) -> Option<&PropertyCounters> {
        match tag {
            TAG_ID => Some(&self.id),
            TAG_DUID => Some(&self.duid),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_is_stable_for_kernel_debugger() {
        assert_eq!(core::mem::size_of::<PropertyCounters>(), 72);
        assert_eq!(core::mem::size_of::<NamespaceIdCounters>(), 144);
        assert_eq!(core::mem::offset_of!(NamespaceIdCounters, duid), 72);
        assert_eq!(core::mem::offset_of!(PropertyCounters, last_status), 64);
    }
    #[test]
    fn counts_requests_success_fail_invalid_and_reject_separately() {
        let c = NamespaceIdCounters::empty();
        assert!(c.for_tag(0).is_none());
        let p = c.for_tag(TAG_ID).unwrap();
        p.on_request();
        p.on_request();
        p.on_completion(0, true);
        p.on_patch(false);
        p.on_completion(0, false);
        p.on_completion(-1, false);
        assert_eq!(p.requests.load(Ordering::Relaxed), 2);
        assert_eq!(p.completions.load(Ordering::Acquire), 3);
        assert_eq!(p.successful.load(Ordering::Relaxed), 2);
        assert_eq!(p.failed.load(Ordering::Relaxed), 1);
        assert_eq!(p.invalid_output.load(Ordering::Relaxed), 1);
        assert_eq!(p.patch_attempts.load(Ordering::Acquire), 1);
        assert_eq!(p.rejected.load(Ordering::Relaxed), 1);
        assert_eq!(p.patched.load(Ordering::Relaxed), 0);
        assert_eq!(p.last_status.load(Ordering::Relaxed), 0xffff_ffff);
        c.duid.on_request();
        c.duid.on_completion(0, true);
        c.duid.on_patch(true);
        assert_eq!(c.duid.patched.load(Ordering::Relaxed), 1);
    }
}
