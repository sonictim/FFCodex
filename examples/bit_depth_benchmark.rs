use std::time::Instant;

/// Simple test to benchmark bit depth conversion optimizations
pub fn test_bit_depth_optimization() {
    // Generate test data
    let sample_count = 100_000;
    let test_data: Vec<f32> = (0..sample_count)
        .map(|i| (i as f32 / sample_count as f32 * 2.0 - 1.0))
        .collect();

    println!("Testing bit depth conversion with {} samples", sample_count);

    // Test original algorithm
    let start = Instant::now();
    let _result1 = change_bit_depth_original(&test_data, 32, 16, true);
    let time1 = start.elapsed();
    println!("Original algorithm: {:?}", time1);

    // Test optimized algorithm
    let start = Instant::now();
    let _result2 = change_bit_depth_optimized(&test_data, 32, 16, true);
    let time2 = start.elapsed();
    println!("Optimized algorithm: {:?}", time2);

    let speedup = time1.as_nanos() as f64 / time2.as_nanos() as f64;
    println!("Speedup: {:.2}x", speedup);
}

/// Original implementation (using iterator chain)
fn change_bit_depth_original(
    input: &[f32],
    src_bits: u32,
    dst_bits: u32,
    dither: bool,
) -> Vec<f32> {
    if src_bits == dst_bits {
        return input.to_vec();
    }

    let dst_max_value = (1 << (dst_bits - 1)) as f32 - 1.0;
    let dither_amplitude = if dither && dst_bits < src_bits {
        1.0 / dst_max_value
    } else {
        0.0
    };

    input
        .iter()
        .map(|&sample| {
            let dithered_sample = if dither_amplitude > 0.0 {
                let dither_value = rand::random::<f32>() - 0.5 + rand::random::<f32>() - 0.5;
                sample + dither_value * dither_amplitude
            } else {
                sample
            };

            let quantized = (dithered_sample * dst_max_value).round() / dst_max_value;
            quantized.clamp(-1.0, 1.0)
        })
        .collect()
}

/// Optimized implementation
fn change_bit_depth_optimized(
    input: &[f32],
    src_bits: u32,
    dst_bits: u32,
    dither: bool,
) -> Vec<f32> {
    if src_bits == dst_bits {
        return input.to_vec();
    }

    let dst_max_value = (1 << (dst_bits - 1)) as f32 - 1.0;
    let inv_dst_max_value = 1.0 / dst_max_value;
    let should_dither = dither && dst_bits < src_bits;

    if should_dither {
        change_bit_depth_with_dither_fast(input, dst_max_value, inv_dst_max_value)
    } else {
        change_bit_depth_no_dither_fast(input, dst_max_value, inv_dst_max_value)
    }
}

fn change_bit_depth_no_dither_fast(
    input: &[f32],
    dst_max_value: f32,
    inv_dst_max_value: f32,
) -> Vec<f32> {
    use rayon::prelude::*;

    if input.len() > 10_000 {
        input
            .par_iter()
            .map(|&sample| {
                let quantized = (sample * dst_max_value).round() * inv_dst_max_value;
                quantized.clamp(-1.0, 1.0)
            })
            .collect()
    } else {
        let mut output = Vec::with_capacity(input.len());
        for &sample in input {
            let quantized = (sample * dst_max_value).round() * inv_dst_max_value;
            output.push(quantized.clamp(-1.0, 1.0));
        }
        output
    }
}

fn change_bit_depth_with_dither_fast(
    input: &[f32],
    dst_max_value: f32,
    inv_dst_max_value: f32,
) -> Vec<f32> {
    let dither_amplitude = inv_dst_max_value;
    const BATCH_SIZE: usize = 1024;
    let mut output = Vec::with_capacity(input.len());

    for chunk in input.chunks(BATCH_SIZE) {
        // Pre-generate dither values for the chunk
        let dither_values: Vec<f32> = (0..chunk.len())
            .map(|_| {
                let dither = (rand::random::<f32>() - 0.5) + (rand::random::<f32>() - 0.5);
                dither * dither_amplitude
            })
            .collect();

        // Process chunk
        for (i, &sample) in chunk.iter().enumerate() {
            let dithered_sample = sample + dither_values[i];
            let quantized = (dithered_sample * dst_max_value).round() * inv_dst_max_value;
            output.push(quantized.clamp(-1.0, 1.0));
        }
    }

    output
}

fn main() {
    test_bit_depth_optimization();
}
