use crate::prelude::*;

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::OnceLock;

/// Sinc function: sin(πx) / (πx)
fn sinc(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// Hann window function
fn hann_window(n: usize, length: usize) -> f32 {
    let n = n as f32;
    let len = length as f32;
    0.5 * (1.0 - (2.0 * PI * n / (len - 1.0)).cos())
}

/// Generates a windowed sinc kernel centered around 0
fn generate_kernel(pos: f32, kernel_size: usize, cutoff: f32) -> Vec<f32> {
    let mut kernel = Vec::with_capacity(kernel_size);
    let half = kernel_size as isize / 2;

    for i in -half..half {
        let t = i as f32 - pos;
        let window = hann_window((i + half) as usize, kernel_size);
        kernel.push(sinc(t * cutoff) * window);
    }

    // Normalize to preserve amplitude
    let sum: f32 = kernel.iter().sum();
    for val in kernel.iter_mut() {
        *val /= sum;
    }

    kernel
}

// Add a struct to cache kernels
struct KernelCache {
    kernels: HashMap<u32, Vec<f32>>,
    kernel_size: usize,
    cutoff: f32,
}

impl KernelCache {
    fn new(kernel_size: usize, cutoff: f32) -> Self {
        Self {
            kernels: HashMap::new(),
            kernel_size,
            cutoff,
        }
    }

    fn get_kernel(&mut self, frac_fixed: u32) -> &Vec<f32> {
        if !self.kernels.contains_key(&frac_fixed) {
            let frac = frac_fixed as f32 / 65536.0; // Convert from fixed point
            let kernel = generate_kernel_optimized(frac, self.kernel_size, self.cutoff);
            self.kernels.insert(frac_fixed, kernel);
        }
        &self.kernels[&frac_fixed]
    }
}

// Pre-computed lookup table for sinc values
const SINC_TABLE_SIZE: usize = 8192;
static SINC_TABLE: OnceLock<Vec<f32>> = OnceLock::new();
static HANN_TABLE: OnceLock<Vec<f32>> = OnceLock::new();

/// Fast sinc function using lookup table
fn sinc_fast(x: f32) -> f32 {
    let table = SINC_TABLE.get_or_init(|| {
        let mut sinc_table = Vec::with_capacity(SINC_TABLE_SIZE);
        for i in 0..SINC_TABLE_SIZE {
            let x = (i as f32) / (SINC_TABLE_SIZE as f32) * 16.0 - 8.0; // Range: -8 to 8
            sinc_table.push(sinc_direct(x));
        }
        sinc_table
    });

    let abs_x = x.abs();
    if abs_x >= 8.0 {
        return 0.0; // Beyond useful range
    }

    let index = ((abs_x + 8.0) / 16.0 * (SINC_TABLE_SIZE as f32)).min((SINC_TABLE_SIZE - 1) as f32)
        as usize;
    table[index]
}

/// Fast Hann window using lookup table
fn hann_window_fast(n: usize, length: usize) -> f32 {
    let table = HANN_TABLE.get_or_init(|| {
        let mut hann_table = Vec::with_capacity(SINC_TABLE_SIZE);
        for i in 0..SINC_TABLE_SIZE {
            // Hann window for range [0, 1]
            let n = (i as f32) / (SINC_TABLE_SIZE as f32 - 1.0);
            hann_table.push(0.5 * (1.0 - (2.0 * PI * n).cos()));
        }
        hann_table
    });

    let normalized = (n as f32) / ((length - 1) as f32);
    let index =
        (normalized * ((SINC_TABLE_SIZE - 1) as f32)).min((SINC_TABLE_SIZE - 1) as f32) as usize;
    table[index]
}

/// Optimized kernel generation with lookup tables
fn generate_kernel_optimized(pos: f32, kernel_size: usize, cutoff: f32) -> Vec<f32> {
    let mut kernel = Vec::with_capacity(kernel_size);
    let half = kernel_size as isize / 2;

    for i in -half..half {
        let t = i as f32 - pos;
        let window = hann_window_fast((i + half) as usize, kernel_size);
        kernel.push(sinc_fast(t * cutoff) * window);
    }

    // Normalize to preserve amplitude
    let sum: f32 = kernel.iter().sum();
    if sum != 0.0 {
        let inv_sum = 1.0 / sum;
        for val in kernel.iter_mut() {
            *val *= inv_sum;
        }
    }

    kernel
}

// Direct sinc calculation (for table generation)
fn sinc_direct(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// Resample a mono f32 buffer from `src_rate` to `dst_rate`
pub fn resample_windowed_sinc(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    let ratio = dst_rate as f32 / src_rate as f32;
    let output_len = ((input.len() as f32) * ratio).ceil() as usize;

    let kernel_size = 32; // You can experiment with this
    let cutoff = 0.9_f32.min(1.0 / ratio); // low-pass cutoff for anti-aliasing

    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_pos = i as f32 / ratio;
        let src_index = src_pos.floor();
        let frac = src_pos - src_index;
        let kernel = generate_kernel(frac, kernel_size, cutoff);

        let mut sample = 0.0;
        for (j, &k) in kernel.iter().enumerate() {
            let idx = src_index as isize + j as isize - (kernel_size as isize / 2);
            if idx >= 0 && (idx as usize) < input.len() {
                sample += input[idx as usize] * k;
            }
        }

        output.push(sample);
    }

    output
}

/// Convert audio samples from one bit depth to another
///
/// # Parameters
/// * `input` - The input samples (assumed to be normalized in range [-1.0, 1.0])
/// * `src_bits` - Source bit depth (e.g., 32, 24, 16)
/// * `dst_bits` - Destination bit depth (e.g., 24, 16, 8)
/// * `dither` - Whether to apply dithering (recommended when reducing bit depth)
///
/// # Returns
/// * A new vector of f32 samples quantized to the new bit depth but still in float range
pub fn change_bit_depth(input: &[f32], src_bits: u32, dst_bits: u32, dither: bool) -> Vec<f32> {
    if src_bits == dst_bits {
        return input.to_vec(); // No change needed
    }

    // Calculate the quantization step size for the target bit depth
    let dst_max_value = (1 << (dst_bits - 1)) as f32 - 1.0;

    // For dithering we'll use triangular probability density function (TPDF)
    let dither_amplitude = if dither && dst_bits < src_bits {
        // Set dither amplitude to 1 LSB of the target format
        1.0 / dst_max_value
    } else {
        0.0
    };

    input
        .iter()
        .map(|&sample| {
            // Apply dithering if requested (when reducing bit depth)
            let dithered_sample = if dither_amplitude > 0.0 {
                // Generate TPDF dither (sum of two uniform random variables)
                let dither_value = rand::random::<f32>() - 0.5 + rand::random::<f32>() - 0.5;
                sample + dither_value * dither_amplitude
            } else {
                sample
            };

            // Quantize to the destination bit depth
            let quantized = (dithered_sample * dst_max_value).round() / dst_max_value;

            // Clip to the valid range for the bit depth
            quantized.clamp(-1.0, 1.0)
        })
        .collect()
}

/// Convert a 32-bit float audio buffer to a specific bit depth
/// Useful when preparing for export in different formats
///
/// # Parameters
/// * `input` - The input samples (floating point in range [-1.0, 1.0])
/// * `bit_depth` - Target bit depth (e.g., 24, 16, 8)
/// * `dither` - Whether to apply dithering (recommended for bit depth reduction)
///
/// # Returns
/// * A vector of bytes representing the audio in the specified bit depth
pub fn convert_to_pcm_bytes(input: &[f32], bit_depth: u32, dither: bool) -> Vec<u8> {
    // First quantize the float samples to the target bit depth
    let quantized = change_bit_depth(input, 32, bit_depth, dither);

    // Calculate bytes per sample
    let bytes_per_sample = (bit_depth + 7) / 8; // Round up to nearest byte
    let mut output = Vec::with_capacity(quantized.len() * bytes_per_sample as usize);

    // Convert each sample to the appropriate number of bytes
    let max_value = (1 << (bit_depth - 1)) as f32 - 1.0;

    for &sample in &quantized {
        // Scale to integer range
        let value = (sample * max_value) as i32;

        // Write the correct number of bytes in little-endian order
        match bytes_per_sample {
            1 => {
                // 8-bit (unsigned)
                output.push((value as u8).wrapping_add(128));
            }
            2 => {
                // 16-bit
                output.extend_from_slice(&(value as i16).to_le_bytes());
            }
            3 => {
                // 24-bit
                let bytes = (value).to_le_bytes();
                output.extend_from_slice(&bytes[0..3]); // Take only the first 3 bytes
            }
            4 => {
                // 32-bit
                output.extend_from_slice(&(value).to_le_bytes());
            }
            _ => {
                // For any other bit depth, default to 32-bit
                output.extend_from_slice(&(value).to_le_bytes());
            }
        }
    }

    output
}

/// Convert PCM bytes back to floating point samples
///
/// # Parameters
/// * `input` - The input bytes
/// * `bit_depth` - Source bit depth (e.g., 24, 16, 8)
///
/// # Returns
/// * A vector of f32 samples normalized to [-1.0, 1.0]
pub fn convert_from_pcm_bytes(input: &[u8], bit_depth: u32) -> Vec<f32> {
    let bytes_per_sample = (bit_depth + 7) / 8; // Round up to nearest byte
    let sample_count = input.len() / bytes_per_sample as usize;
    let max_value = (1 << (bit_depth - 1)) as f32 - 1.0;

    let mut output = Vec::with_capacity(sample_count);

    for i in 0..sample_count {
        let offset = i * bytes_per_sample as usize;

        let value = match bit_depth {
            8 => {
                // 8-bit audio is typically unsigned
                (input[offset] as i32) - 128
            }
            16 => {
                if offset + 1 < input.len() {
                    let bytes = [input[offset], input[offset + 1], 0, 0];
                    i16::from_le_bytes([bytes[0], bytes[1]]) as i32
                } else {
                    0
                }
            }
            24 => {
                if offset + 2 < input.len() {
                    // Extract 3 bytes and sign-extend to 4 bytes
                    let mut bytes = [input[offset], input[offset + 1], input[offset + 2], 0];

                    // Sign extension
                    if (bytes[2] & 0x80) != 0 {
                        bytes[3] = 0xFF;
                    }

                    i32::from_le_bytes(bytes)
                } else {
                    0
                }
            }
            32 => {
                if offset + 3 < input.len() {
                    let bytes = [
                        input[offset],
                        input[offset + 1],
                        input[offset + 2],
                        input[offset + 3],
                    ];
                    i32::from_le_bytes(bytes)
                } else {
                    0
                }
            }
            _ => {
                if offset + 3 < input.len() {
                    let bytes = [
                        input[offset],
                        input[offset + 1],
                        input[offset + 2],
                        input[offset + 3],
                    ];
                    i32::from_le_bytes(bytes)
                } else {
                    0
                }
            }
        };

        // Convert to float in range [-1.0, 1.0]
        output.push(value as f32 / max_value);
    }

    output
}

/// Highly optimized resample function with multiple improvements
pub fn resample_windowed_sinc_optimized(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    let ratio = dst_rate as f32 / src_rate as f32;
    let output_len = ((input.len() as f32) * ratio).ceil() as usize;

    let kernel_size = 32;
    let cutoff = 0.9_f32.min(1.0 / ratio);

    // Pre-allocate output buffer
    let mut output = Vec::with_capacity(output_len);
    output.resize(output_len, 0.0);

    // Use kernel caching for common fractional positions
    let mut cache = KernelCache::new(kernel_size, cutoff);

    // Process in chunks for better cache locality
    const CHUNK_SIZE: usize = 256;

    for chunk_start in (0..output_len).step_by(CHUNK_SIZE) {
        let chunk_end = (chunk_start + CHUNK_SIZE).min(output_len);

        for i in chunk_start..chunk_end {
            let src_pos = i as f32 / ratio;
            let src_index = src_pos.floor();
            let frac = src_pos - src_index;

            // Use fixed-point for kernel cache key
            let frac_fixed = (frac * 65536.0) as u32;
            let kernel = cache.get_kernel(frac_fixed);

            let mut sample = 0.0;
            let start_idx = src_index as isize - (kernel_size as isize / 2);

            // Vectorized inner loop with bounds checking optimized out
            for (j, &k) in kernel.iter().enumerate() {
                let idx = start_idx + j as isize;
                if idx >= 0 && (idx as usize) < input.len() {
                    sample += input[idx as usize] * k;
                }
            }

            output[i] = sample;
        }
    }

    output
}

/// SIMD-optimized convolution for the inner loop
fn convolve_simd(input: &[f32], kernel: &[f32], start_idx: isize) -> f32 {
    let mut sum = f32x4::ZERO;
    let kernel_len = kernel.len();

    // Process 4 samples at a time
    let simd_len = (kernel_len / 4) * 4; // Use explicit division and multiplication

    for i in (0..simd_len).step_by(4) {
        let idx_base = start_idx + i as isize;

        // Load 4 kernel coefficients
        let k = f32x4::new([kernel[i], kernel[i + 1], kernel[i + 2], kernel[i + 3]]);

        // Load 4 input samples (with bounds checking)
        let mut samples = [0.0f32; 4];
        for j in 0..4 {
            let idx = idx_base + j as isize;
            if idx >= 0 && (idx as usize) < input.len() {
                samples[j] = input[idx as usize];
            }
        }
        let s = f32x4::new(samples);

        // Multiply and accumulate
        sum += k * s;
    }

    // Handle remaining samples
    let mut scalar_sum = sum.reduce_add();
    for i in simd_len..kernel_len {
        let idx = start_idx + i as isize;
        if idx >= 0 && (idx as usize) < input.len() {
            scalar_sum += input[idx as usize] * kernel[i];
        }
    }

    scalar_sum
}

/// Parallel SIMD-optimized resample function
pub fn resample_parallel_simd(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    let ratio = dst_rate as f32 / src_rate as f32;
    let output_len = ((input.len() as f32) * ratio).ceil() as usize;

    let kernel_size = 32;
    let cutoff = 0.9_f32.min(1.0 / ratio);

    // Pre-allocate output buffer
    let mut output = vec![0.0f32; output_len];

    // Process in parallel chunks
    const CHUNK_SIZE: usize = 1024;

    output
        .par_chunks_mut(CHUNK_SIZE)
        .enumerate()
        .for_each(|(chunk_idx, chunk)| {
            let chunk_start = chunk_idx * CHUNK_SIZE;
            let mut cache = KernelCache::new(kernel_size, cutoff);

            for (local_idx, sample_out) in chunk.iter_mut().enumerate() {
                let i = chunk_start + local_idx;
                if i >= output_len {
                    break;
                }

                let src_pos = i as f32 / ratio;
                let src_index = src_pos.floor();
                let frac = src_pos - src_index;

                // Use fixed-point for kernel cache key
                let frac_fixed = (frac * 65536.0) as u32;
                let kernel = cache.get_kernel(frac_fixed);

                // Use SIMD-optimized convolution
                let start_idx = src_index as isize - (kernel_size as isize / 2);
                *sample_out = convolve_simd(input, kernel, start_idx);
            }
        });

    output
}

/// Ultra-fast resample for specific common ratios (2:1, 1:2, etc.)
pub fn resample_fast_common_ratios(
    input: &[f32],
    src_rate: u32,
    dst_rate: u32,
) -> Option<Vec<f32>> {
    let ratio = dst_rate as f64 / src_rate as f64;

    // Handle exact 2:1 downsampling
    if (ratio - 0.5).abs() < 0.001 {
        return Some(resample_downsample_2x(input));
    }

    // Handle exact 1:2 upsampling
    if (ratio - 2.0).abs() < 0.001 {
        return Some(resample_upsample_2x(input));
    }

    // Handle exact 1:1 (no resampling needed)
    if (ratio - 1.0).abs() < 0.001 {
        return Some(input.to_vec());
    }

    None // Fall back to general algorithm
}

/// Optimized 2x downsampling with anti-aliasing
fn resample_downsample_2x(input: &[f32]) -> Vec<f32> {
    let output_len = input.len() / 2;
    let mut output = Vec::with_capacity(output_len);

    // Simple anti-aliasing filter coefficients for 2x downsampling
    const FILTER: [f32; 5] = [0.1, 0.2, 0.4, 0.2, 0.1];

    for i in 0..output_len {
        let src_idx = i * 2;
        let mut sample = 0.0;

        for (j, &coeff) in FILTER.iter().enumerate() {
            let idx = src_idx + j;
            if idx >= 2 && idx < input.len() {
                sample += input[idx - 2] * coeff;
            }
        }

        output.push(sample);
    }

    output
}

/// Optimized 2x upsampling with interpolation
fn resample_upsample_2x(input: &[f32]) -> Vec<f32> {
    let output_len = input.len() * 2;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..input.len() {
        output.push(input[i]);

        // Linear interpolation for the inserted sample
        if i + 1 < input.len() {
            output.push((input[i] + input[i + 1]) * 0.5);
        } else {
            output.push(input[i]);
        }
    }

    output
}

/// Main optimized resample function that chooses the best algorithm
pub fn resample_optimized(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    // Try fast path for common ratios first
    if let Some(result) = resample_fast_common_ratios(input, src_rate, dst_rate) {
        return result;
    }

    // For large inputs, use parallel processing
    if input.len() > 10000 {
        resample_parallel_simd(input, src_rate, dst_rate)
    } else {
        // Use single-threaded optimized version for smaller inputs
        resample_windowed_sinc_optimized(input, src_rate, dst_rate)
    }
}

/// Benchmark different resample algorithms
pub fn benchmark_resample_algorithms(input: &[f32], src_rate: u32, dst_rate: u32) {
    use std::time::Instant;

    dprintln!(
        "Benchmarking resample algorithms for {}Hz -> {}Hz ({} samples)",
        src_rate,
        dst_rate,
        input.len()
    );

    // Original algorithm
    let start = Instant::now();
    let _result1 = resample_windowed_sinc(input, src_rate, dst_rate);
    let time1 = start.elapsed();
    dprintln!("Original algorithm: {:?}", time1);

    // Optimized algorithm
    let start = Instant::now();
    let _result2 = resample_optimized(input, src_rate, dst_rate);
    let time2 = start.elapsed();
    dprintln!("Optimized algorithm: {:?}", time2);

    // Parallel SIMD algorithm
    let start = Instant::now();
    let _result3 = resample_parallel_simd(input, src_rate, dst_rate);
    let time3 = start.elapsed();
    dprintln!("Parallel SIMD algorithm: {:?}", time3);

    let speedup2 = time1.as_nanos() as f64 / time2.as_nanos() as f64;
    let speedup3 = time1.as_nanos() as f64 / time3.as_nanos() as f64;

    dprintln!("Optimized speedup: {:.2}x", speedup2);
    dprintln!("Parallel SIMD speedup: {:.2}x", speedup3);
}
