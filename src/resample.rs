use std::f32::consts::PI;

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
    let mut rng = rand::thread_rng();
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
            quantized.max(-1.0).min(1.0)
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
                let bytes = (value as i32).to_le_bytes();
                output.extend_from_slice(&bytes[0..3]); // Take only the first 3 bytes
            }
            4 | _ => {
                // 32-bit
                output.extend_from_slice(&(value as i32).to_le_bytes());
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
            32 | _ => {
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
