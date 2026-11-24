//! Exercise real WASAPI apartment lifetime without recording or playing sound.
#[cfg(windows)]
#[test]
fn default_audio_device_survives_sequential_and_concurrent_worker_teardown() {
    use cpal::traits::HostTrait as _;
    let probe = || {
        let host = cpal::default_host();
        let _output = host.default_output_device();
    };
    for _ in 0..16 {
        std::thread::spawn(probe)
            .join()
            .expect("audio probe thread");
    }
    let workers = (0..16)
        .map(|_| std::thread::spawn(probe))
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().expect("concurrent audio probe thread");
    }
}
