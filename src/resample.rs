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
