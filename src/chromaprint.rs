use crate::chromaprint_bindings::{CHROMAPRINT_ALGORITHM_DEFAULT, Chromaprint};
use crate::prelude::*;
use base64::{Engine as _, engine::general_purpose};

impl Codex {
    pub fn get_chromaprint_fingerprint(&mut self) -> R<String> {
        // This is fine since your implementation checks internally

        if self.buffer.format.bits_per_sample() > 24 {
            println!("{} bith depth is not supported", self.get_filename());
            self.change_bit_depth(24)
        }

        let sample_rate = if self.buffer.sample_rate == 44100 {
            44100
        } else {
            48000
        };

        self.resample(sample_rate);

        const MIN_SAMPLES_PER_CHANNEL: usize = 24000; // 0.5 seconds at 48kHz per channel

        // Check if we have enough samples in any channel
        let has_enough_samples = self
            .buffer
            .data
            .iter()
            .any(|ch| ch.len() >= MIN_SAMPLES_PER_CHANNEL);

        if !has_enough_samples {
            println!("Audio is too short for Chromaprint, using PCM hash instead");
            return self.generate_pcm_hash();
        }

        let num_channels = if self.buffer.channels > 1 { 2 } else { 1 };

        let samples = if self.buffer.channels > 1 {
            interleave_stereo(&self.buffer.data)
        } else {
            single_channel(&self.buffer.data)
        };

        // Try Chromaprint fingerprinting
        let c = Chromaprint::new(CHROMAPRINT_ALGORITHM_DEFAULT);
        if c.start(sample_rate as i32, num_channels) {
            c.feed(&samples);
            c.finish();

            // if let Some(fingerprint) = c.get_fingerprint() {
            //     println!(
            //         "Success! Generated Chromaprint fingerprint for: {}",
            //         self.get_filename()
            //     );
            //     return Ok(fingerprint);
            // }
            if let Some(fingerprint) = c.get_raw_fingerprint() {
                println!(
                    "Generated raw fingerprint for: {} size; {}",
                    self.get_filename(),
                    fingerprint.len()
                );
                // Convert Vec<i32> to bytes before encoding
                let bytes: Vec<u8> = fingerprint.iter().flat_map(|&x| x.to_le_bytes()).collect();
                let encoded = general_purpose::STANDARD.encode(&bytes);
                if !encoded.is_empty() {
                    return Ok(encoded);
                }
            }
        }

        // Fallback to PCM hash
        self.generate_pcm_hash()
    }

    // Extract PCM hash generation to a separate method
    fn generate_pcm_hash(&self) -> R<String> {
        use sha2::{Digest, Sha256};

        let samples = if self.buffer.channels > 1 {
            interleave_stereo(&self.buffer.data)
        } else {
            single_channel(&self.buffer.data)
        };

        if samples.is_empty() {
            return Err(anyhow::anyhow!(
                "Failed to generate any fingerprint: no samples available"
            ));
        }

        let mut hasher = Sha256::new();

        // Convert samples to bytes in larger chunks
        for chunk in samples.chunks(4096) {
            // Create a byte buffer for this chunk
            let mut bytes = Vec::with_capacity(chunk.len() * 2);
            for &sample in chunk {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            hasher.update(&bytes);
        }

        let hash = hasher.finalize();
        println!("Success! Generated PCM hash for: {}", self.get_filename());
        let fingerprint = format!("PCM:{}", general_purpose::STANDARD.encode(hash));
        Ok(fingerprint)
    }
}

// Helper functions to convert audio data remain the same
fn interleave_stereo(channels: &[Vec<f32>]) -> Vec<i16> {
    let left = &channels[0];
    let right = &channels[1];

    // Pre-allocate the exact capacity needed
    let len = left.len().min(right.len());
    let mut interleaved = Vec::with_capacity(len * 2);

    // Use iterators with zip() instead of indexed for loop
    left.iter()
        .zip(right.iter())
        .take(len)
        .for_each(|(&l, &r)| {
            let l_sample = (l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            let r_sample = (r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            interleaved.push(l_sample);
            interleaved.push(r_sample);
        });

    interleaved
}

fn single_channel(channels: &[Vec<f32>]) -> Vec<i16> {
    if channels.is_empty() {
        return Vec::new();
    }

    let channel = &channels[0];
    let scale = i16::MAX as f32;

    // Use map+collect instead of push in a loop
    channel
        .iter()
        .map(|&sample| (sample.clamp(-1.0, 1.0) * scale) as i16)
        .collect()
}
