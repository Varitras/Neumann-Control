//! The CPU time this process has used, all threads, for the detailed log's frame summary.

/// Seconds of CPU time (user and kernel) the process has used since it started; `None` where
/// the system does not tell.
#[cfg(windows)]
pub fn cpu_seconds() -> Option<f64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    /// A FILETIME counts 100 ns steps.
    const STEPS_PER_SECOND: f64 = 10_000_000.0;
    let steps = |time: FILETIME| {
        (u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)) as f64
    };
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    let told = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if told == 0 {
        return None;
    }
    Some((steps(kernel) + steps(user)) / STEPS_PER_SECOND)
}

#[cfg(not(windows))]
pub fn cpu_seconds() -> Option<f64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn work_shows_in_the_process_cpu_time() {
        let before = cpu_seconds().unwrap();
        let mut sum = 0u64;
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_millis(100) {
            sum = std::hint::black_box(sum.wrapping_add(1));
        }
        let after = cpu_seconds().unwrap();
        assert!(after > before, "{before} → {after}");
    }
}
