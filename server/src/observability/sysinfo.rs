//! Lightweight Linux system statistics read from `/proc`.
//!
//! Pure `std` (no external crates). All readers return `Option`/`Result` and
//! never panic, so a missing or unexpected `/proc` (e.g. a non-Linux dev host)
//! simply yields `None` rather than breaking metrics collection.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Clock ticks per second (`sysconf(_SC_CLK_TCK)`). 100 on every mainstream
/// Linux; the server only ever runs on Linux (container + CI).
const USER_HZ: f64 = 100.0;

/// Resident set size of this process in bytes (`/proc/self/status` `VmRSS`).
pub fn process_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

/// System memory `(total_bytes, available_bytes)` from `/proc/meminfo`.
pub fn system_memory_bytes() -> Option<(u64, u64)> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut available = None;
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok();
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok();
        }
        if total.is_some() && available.is_some() {
            break;
        }
    }
    Some((total? * 1024, available? * 1024))
}

/// 1-minute load average (first field of `/proc/loadavg`).
pub fn load_average_1m() -> Option<f64> {
    let loadavg = std::fs::read_to_string("/proc/loadavg").ok()?;
    loadavg.split_whitespace().next()?.parse().ok()
}

/// Number of open file descriptors held by this process.
pub fn open_fds() -> Option<u64> {
    Some(std::fs::read_dir("/proc/self/fd").ok()?.count() as u64)
}

/// Number of OS threads in this process (`/proc/self/status` `Threads`).
pub fn thread_count() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Threads:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// Total CPU time (user + system) consumed by this process, in clock ticks.
/// Fields 14 (utime) and 15 (stime) of `/proc/self/stat`. The comm field
/// (field 2) may contain spaces/parens, so parse after the final `)`.
fn process_cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // After ')' the next field is `state` (index 0 here = field 3). utime is
    // field 14 → index 11, stime is field 15 → index 12.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

/// Cache of the most recently computed CPU percent, so a point-in-time reader
/// (the admin summary) can report it without needing two spaced samples.
/// Stored as `f64` bits; `NaN` means "not yet sampled".
static LAST_CPU_PCT: AtomicU64 = AtomicU64::new(0x7ff8_0000_0000_0000); // NaN bits

/// Instantaneous process CPU usage percent.
///
/// Computed from the CPU-time delta since the previous call. Returns `None` on
/// the first call (establishes a baseline) and caches every computed value for
/// [`last_cpu_percent`].
///
/// Intended to be called on a fixed cadence (the metric-export interval), where
/// the delta window equals the export interval and the result is a true rate.
pub fn process_cpu_percent() -> Option<f64> {
    static PREV: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

    let ticks = process_cpu_ticks()?;
    let now = Instant::now();
    let mut guard = PREV.lock().ok()?;
    let pct = match *guard {
        Some((prev_ticks, prev_t)) => {
            let dt = now.duration_since(prev_t).as_secs_f64();
            if dt <= 0.0 {
                None
            } else {
                let d_ticks = ticks.saturating_sub(prev_ticks) as f64;
                Some((d_ticks / USER_HZ) / dt * 100.0)
            }
        }
        None => None,
    };
    *guard = Some((ticks, now));
    if let Some(p) = pct {
        LAST_CPU_PCT.store(p.to_bits(), Ordering::Relaxed);
    }
    pct
}

/// The most recently computed CPU percent (see [`process_cpu_percent`]), or
/// `None` if it has not been sampled yet.
pub fn last_cpu_percent() -> Option<f64> {
    let v = f64::from_bits(LAST_CPU_PCT.load(Ordering::Relaxed));
    if v.is_nan() {
        None
    } else {
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn reads_proc_values() {
        assert!(process_rss_bytes().is_some_and(|b| b > 0));
        assert!(thread_count().is_some_and(|t| t >= 1));
        assert!(open_fds().is_some_and(|f| f >= 1));
        assert!(load_average_1m().is_some_and(|l| l >= 0.0));
        let (total, avail) = system_memory_bytes().expect("meminfo");
        assert!(total > 0 && avail <= total);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn cpu_percent_needs_two_samples() {
        // First call establishes a baseline (None); a later call yields a value.
        let _ = process_cpu_percent();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Busy a little so there is measurable CPU time.
        let mut x: u64 = 0;
        for i in 0..2_000_000 {
            x = x.wrapping_add(i);
        }
        std::hint::black_box(x);
        let pct = process_cpu_percent();
        assert!(pct.is_some_and(|p| (0.0..=1000.0).contains(&p)));
        assert!(last_cpu_percent().is_some());
    }
}
