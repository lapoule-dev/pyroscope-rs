#[cfg(all(
    feature = "backend-pprof-rs",
    target_os = "macos",
    target_arch = "aarch64"
))]
mod tests {
    use pyroscope::backend::{pprof_backend, BackendConfig, PprofConfig, ReportData};
    use std::time::Duration;

    /// SIGPROF delivered to a thread whose pthread signature is not valid
    /// must not kill the process.
    ///
    /// A fresh GCD workqueue thread starts in `start_wqthread` with a zeroed
    /// pthread structure; `_pthread_wqthread_setup` signs it only a few
    /// instructions later. `ITIMER_PROF` lands on whichever thread is on CPU,
    /// so a sample can fall in that window. On arm64e `pthread_self()`
    /// authenticates the signature (`autdb`) and executes `brk #0xc473` when
    /// it does not check out: SIGTRAP, "pointer authentication trap DB",
    /// from inside the signal handler.
    ///
    /// The window is a handful of instructions wide, so the test opens it on
    /// purpose: it zeroes the signature word of its own pthread structure
    /// (the state such a thread is born in), burns CPU so the profiling timer
    /// fires on this thread, then restores the word. Nothing in the spin
    /// touches pthread state.
    #[test]
    fn test_sigprof_on_thread_with_unsigned_pthread() {
        let mut backend =
            pprof_backend(PprofConfig { sample_rate: 1000 }, BackendConfig::default())
                .initialize()
                .expect("failed to initialize pprof backend");
        std::thread::sleep(Duration::from_millis(50));

        let sampled = std::thread::spawn(|| {
            // `pthread_t` on Darwin is the address of `struct _pthread`, whose
            // first word is the signed signature `pthread_self()` checks.
            let sig = unsafe { libc::pthread_self() } as *mut u64;
            let saved = unsafe { std::ptr::read_volatile(sig) };
            unsafe { std::ptr::write_volatile(sig, 0) };
            let mut acc = 0u64;
            for i in 0..1_000_000_000u64 {
                acc = std::hint::black_box(acc.wrapping_mul(6364136223846793005).wrapping_add(i));
            }
            unsafe { std::ptr::write_volatile(sig, saved) };
            acc
        });
        sampled.join().expect("sampled thread panicked");

        let batch = backend.report().expect("failed to dump report");
        let samples: usize = match batch.data {
            ReportData::Reports(reports) => {
                reports.iter().map(|r| r.data.values().sum::<usize>()).sum()
            }
            ReportData::RawPprof(_) => panic!("the pprof backend returns structured reports"),
        };
        eprintln!("{samples} samples");
        assert!(samples > 0, "the profiler recorded no sample");
    }
}
