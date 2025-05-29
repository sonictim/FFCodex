use ffcodex_lib::resample::*;
use std::time::Instant;

fn main() {
    // Create test audio data (1 second of 44.1kHz sine wave)
    let sample_rate = 44100;
    let duration = 1.0; // seconds
    let frequency = 440.0; // A4 note

    let samples = (sample_rate as f32 * duration) as usize;
    let mut input = Vec::with_capacity(samples);

    for i in 0..samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * frequency * t).sin() * 0.5;
        input.push(sample);
    }

    println!("Generated {} samples at {}Hz", input.len(), sample_rate);

    // Test different resampling scenarios
    let test_cases = [
        (44100, 48000), // Common upsampling
        (48000, 44100), // Common downsampling
        (44100, 22050), // 2x downsampling
        (22050, 44100), // 2x upsampling
        (44100, 96000), // High-quality upsampling
    ];

    for (src_rate, dst_rate) in test_cases.iter() {
        println!("\n=== Resampling {}Hz -> {}Hz ===", src_rate, dst_rate);

        // Original algorithm
        let start = Instant::now();
        let result1 = resample_windowed_sinc(&input, *src_rate, *dst_rate);
        let time1 = start.elapsed();
        println!(
            "Original:     {:>8.2}ms ({} samples)",
            time1.as_secs_f64() * 1000.0,
            result1.len()
        );

        // Optimized algorithm
        let start = Instant::now();
        let result2 = resample_optimized(&input, *src_rate, *dst_rate);
        let time2 = start.elapsed();
        println!(
            "Optimized:    {:>8.2}ms ({} samples)",
            time2.as_secs_f64() * 1000.0,
            result2.len()
        );

        // Parallel SIMD algorithm
        let start = Instant::now();
        let result3 = resample_parallel_simd(&input, *src_rate, *dst_rate);
        let time3 = start.elapsed();
        println!(
            "Parallel SIMD:{:>8.2}ms ({} samples)",
            time3.as_secs_f64() * 1000.0,
            result3.len()
        );

        if time1.as_nanos() > 0 {
            let speedup2 = time1.as_nanos() as f64 / time2.as_nanos() as f64;
            let speedup3 = time1.as_nanos() as f64 / time3.as_nanos() as f64;

            println!("Speedup:      {:>8.2}x / {:>8.2}x", speedup2, speedup3);
        }

        // Quick quality check - ensure outputs are similar length
        let expected_len =
            ((input.len() as f32) * (*dst_rate as f32) / (*src_rate as f32)).ceil() as usize;
        println!("Expected len: {} samples", expected_len);
    }
}
