//! Process-level OS memory counters, distinct from diagnostic payload storage.
use serde::Serialize;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub resident_bytes: Option<u64>,
    pub peak_resident_bytes: Option<u64>,
    pub private_bytes: Option<u64>,
    pub source: &'static str,
}

#[cfg(windows)]
pub fn memory() -> Memory {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    counters.cb = std::mem::size_of_val(&counters) as u32;
    // The OS writes exactly cb bytes into this initialized EX structure. The
    // API takes the base pointer and accepts the EX size to include PrivateUsage.
    let success = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
    };
    if success == 0 {
        return Memory {
            source: "unavailable",
            ..Default::default()
        };
    }
    Memory {
        resident_bytes: Some(counters.WorkingSetSize as u64),
        peak_resident_bytes: Some(counters.PeakWorkingSetSize as u64),
        private_bytes: Some(counters.PrivateUsage as u64),
        source: "GetProcessMemoryInfo",
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn memory() -> Memory {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return Memory {
            source: "unavailable",
            ..Default::default()
        };
    };
    let kib = |name: &str| {
        status.lines().find_map(|line| {
            line.strip_prefix(name)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .checked_mul(1024)
        })
    };
    Memory {
        resident_bytes: kib("VmRSS:"),
        peak_resident_bytes: kib("VmHWM:"),
        private_bytes: None, // VmSize is virtual address space, not private commit.
        source: "/proc/self/status",
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
pub fn memory() -> Memory {
    Memory {
        source: "unavailable",
        ..Default::default()
    }
}
