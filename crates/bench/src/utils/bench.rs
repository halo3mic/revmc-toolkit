use std::time::{Instant, Duration};
use tracing::info;


pub fn measure_execution_time<F, R>(mut f: F, warmup_ms: u32, measurement_ms: u32) -> f64
where
    F: FnMut() -> R,
{
    info!("Warming up for {warmup_ms} ms");
    let warm_up_duration = Duration::from_millis(warmup_ms as u64);
    let start = Instant::now();
    let mut warmup_iter = 0;
    loop {
        f();
        if Instant::now() - start > warm_up_duration {
            break;
        }
        warmup_iter += 1;
    }

    let measurement_iter = warmup_iter * measurement_ms / warmup_ms;
    info!("Measuring with {measurement_iter} iterations");
    let start = Instant::now();
    for _ in 0..measurement_iter {
        f();
    }
    let m_duration = (Instant::now() - start).as_nanos();

    m_duration as f64 / measurement_iter as f64
}