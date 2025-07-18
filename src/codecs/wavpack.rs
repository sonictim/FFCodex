//! WavPack codec implementation for FFCodex
//!
//! This module provides complete WavPack (.wv) format support including:
//! - Lossless and hybrid lossy compression
//! - Multi-channel audio support
//! - Comprehensive metadata handling
//! - High-quality encoding and decoding

use crate::prelude::*;
use crate::wavpack_bindings::*;
use memmap2::MmapOptions;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

/// Safe wrapper around WavPack context for decoding
pub struct WavpackDecoder {
    context: *mut WavpackContext,
    temp_file: Option<std::path::PathBuf>,
}

impl WavpackDecoder {
    /// Create a new decoder from file data
    pub fn new(data: &[u8]) -> R<Self> {
        // WavPack C API requires file-based access, so we need to write to a temp file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("ffcodex_wavpack_{}.wv", rand::random::<u32>()));

        std::fs::write(&temp_file, data)
            .map_err(|e| anyhow!("Failed to write temp WavPack file: {}", e))?;

        let temp_path_str = temp_file
            .to_str()
            .ok_or_else(|| anyhow!("Invalid temp file path"))?;

        let c_filename =
            CString::new(temp_path_str).map_err(|e| anyhow!("Invalid filename: {}", e))?;

        let mut error_buffer = [0i8; 256];

        let context = unsafe {
            WavpackOpenFileInput(
                c_filename.as_ptr(),
                error_buffer.as_mut_ptr(),
                OPEN_NORMALIZE | OPEN_WRAPPER | OPEN_TAGS,
                0,
            )
        };

        if context.is_null() {
            let _ = std::fs::remove_file(&temp_file);
            let error_str = unsafe { CStr::from_ptr(error_buffer.as_ptr()) };
            return Err(anyhow!(
                "Failed to open WavPack file: {}",
                error_str.to_string_lossy()
            ));
        }

        Ok(Self {
            context,
            temp_file: Some(temp_file),
        })
    }

    /// Get the number of channels
    pub fn channels(&self) -> u16 {
        unsafe { WavpackGetNumChannels(self.context) as u16 }
    }

    /// Get the sample rate
    pub fn sample_rate(&self) -> u32 {
        unsafe { WavpackGetSampleRate(self.context) }
    }

    /// Get the number of bits per sample
    pub fn bits_per_sample(&self) -> u32 {
        unsafe { WavpackGetBitsPerSample(self.context) as u32 }
    }

    /// Get the total number of samples per channel
    pub fn total_samples(&self) -> u64 {
        unsafe { WavpackGetNumSamples64(self.context) as u64 }
    }

    /// Check if the stream is floating point
    pub fn is_float(&self) -> bool {
        let mode = unsafe { WavpackGetMode(self.context) };
        (mode & MODE_FLOAT) != 0
    }

    /// Check if the stream is lossless
    #[allow(dead_code)]
    pub fn is_lossless(&self) -> bool {
        let mode = unsafe { WavpackGetMode(self.context) };
        (mode & MODE_LOSSLESS) != 0
    }

    /// Decode all samples into an AudioBuffer
    pub fn decode(&mut self) -> R<AudioBuffer> {
        let channels = self.channels();
        let sample_rate = self.sample_rate();
        let total_samples = self.total_samples() as usize;
        let bits_per_sample = self.bits_per_sample();
        let is_float = self.is_float();

        if channels == 0 || sample_rate == 0 {
            return Err(anyhow!(
                "Invalid WavPack file: zero channels or sample rate"
            ));
        }

        // Determine sample format
        let sample_format = match (bits_per_sample, is_float) {
            (8, false) => SampleFormat::U8,
            (16, false) => SampleFormat::I16,
            (24, false) => SampleFormat::I24,
            (32, false) => SampleFormat::I32,
            (32, true) => SampleFormat::F32,
            _ => SampleFormat::I16, // Default fallback
        };

        // Initialize output buffer
        let mut audio_data: Vec<Vec<f32>> =
            vec![Vec::with_capacity(total_samples); channels as usize];

        // Decode in chunks for better memory efficiency
        const CHUNK_SIZE: usize = 4096;
        let mut sample_buffer = vec![0i32; CHUNK_SIZE * channels as usize];
        let mut samples_decoded = 0;

        while samples_decoded < total_samples {
            let samples_to_read = std::cmp::min(CHUNK_SIZE, total_samples - samples_decoded);

            let unpacked = unsafe {
                WavpackUnpackSamples(
                    self.context,
                    sample_buffer.as_mut_ptr(),
                    samples_to_read as uint32_t,
                )
            };

            if unpacked == 0 {
                break; // End of stream or error
            }

            // Convert and de-interleave samples
            self.convert_samples(
                &sample_buffer[..unpacked as usize * channels as usize],
                &mut audio_data,
                bits_per_sample,
                is_float,
                channels,
            )?;

            samples_decoded += unpacked as usize;
        }

        Ok(AudioBuffer {
            sample_rate,
            channels,
            format: sample_format,
            data: audio_data,
        })
    }

    /// Convert interleaved i32 samples to f32 and de-interleave by channel
    fn convert_samples(
        &self,
        interleaved: &[i32],
        output: &mut [Vec<f32>],
        bits_per_sample: u32,
        is_float: bool,
        channels: u16,
    ) -> R<()> {
        let samples_per_channel = interleaved.len() / channels as usize;

        for i in 0..samples_per_channel {
            for ch in 0..channels as usize {
                let sample_idx = i * channels as usize + ch;
                if sample_idx >= interleaved.len() {
                    break;
                }

                let sample_i32 = interleaved[sample_idx];
                let sample_f32 = match (bits_per_sample, is_float) {
                    (8, false) => {
                        // 8-bit is typically unsigned in WAV, but WavPack may store it as signed
                        let unsigned_val = (sample_i32 + 128) as u8;
                        (unsigned_val as f32 / 127.5) - 1.0
                    }
                    (16, false) => sample_i32 as f32 / 32768.0,
                    (24, false) => sample_i32 as f32 / 8388608.0,
                    (32, false) => sample_i32 as f32 / 2147483648.0,
                    (32, true) => {
                        // 32-bit float samples are stored as the bit pattern in the i32
                        f32::from_bits(sample_i32 as u32)
                    }
                    _ => sample_i32 as f32 / 32768.0, // Default to 16-bit conversion
                };

                output[ch].push(sample_f32);
            }
        }

        Ok(())
    }


    /// Detect MIME type from image data
    #[allow(dead_code)]
    fn detect_image_mime_type(data: &[u8]) -> String {
        if data.len() < 8 {
            return "application/octet-stream".to_string();
        }

        if data.starts_with(b"\xFF\xD8\xFF") {
            "image/jpeg".to_string()
        } else if data.starts_with(b"\x89PNG\r\n\x1A\n") {
            "image/png".to_string()
        } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            "image/gif".to_string()
        } else if data.starts_with(b"RIFF") && data[8..12] == *b"WEBP" {
            "image/webp".to_string()
        } else if data.starts_with(b"\x00\x00\x01\x00") {
            "image/x-icon".to_string()
        } else {
            "application/octet-stream".to_string()
        }
    }
}

impl Drop for WavpackDecoder {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe {
                WavpackCloseFile(self.context);
            }
        }

        if let Some(ref temp_file) = self.temp_file {
            let _ = std::fs::remove_file(temp_file);
        }
    }
}

/// Safe wrapper around WavPack context for encoding
pub struct WavpackEncoder {
    context: *mut WavpackContext,
    config: WavpackConfig,
    output_buffer: Vec<u8>,
    temp_file: Option<std::path::PathBuf>,
}

impl WavpackEncoder {
    /// Create a new encoder with the given configuration
    pub fn new(
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u32,
        is_float: bool,
        lossless: bool,
    ) -> R<Self> {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("ffcodex_wavpack_out_{}.wv", rand::random::<u32>()));

        let mut config = WavpackConfig::default();
        config.sample_rate = sample_rate as int32_t;
        config.num_channels = channels as c_int;
        config.bits_per_sample = bits_per_sample as c_int;
        config.bytes_per_sample = ((bits_per_sample + 7) / 8) as c_int;

        // Set flags based on requirements
        if !lossless {
            config.flags |= CONFIG_HYBRID_FLAG;
            config.bitrate = 256.0; // Default hybrid bitrate
        }

        if is_float {
            config.flags |= CONFIG_EXTRA_MODE; // Enable float support
        }

        // Set high quality mode by default
        config.flags |= CONFIG_HIGH_FLAG;

        let output_buffer = Vec::new();

        Ok(Self {
            context: ptr::null_mut(),
            config,
            output_buffer,
            temp_file: Some(temp_file),
        })
    }

    /// Initialize the encoder and begin encoding
    pub fn init(&mut self) -> R<()> {
        // Create output callback that writes to our buffer
        extern "C" fn block_output_callback(
            id: *mut c_void,
            data: *mut c_void,
            bcount: int32_t,
        ) -> c_int {
            if id.is_null() || data.is_null() || bcount <= 0 {
                return 0;
            }

            let encoder = unsafe { &mut *(id as *mut WavpackEncoder) };
            let slice = unsafe { std::slice::from_raw_parts(data as *const u8, bcount as usize) };

            // Add some debug info about what's being written
            let current_len = encoder.output_buffer.len();
            encoder.output_buffer.extend_from_slice(slice);

            if current_len == 0 {
                dprintln!("WavPack output_callback: First write of {} bytes", bcount);
                // Check if this looks like a header (WavPack files start with "wvpk")
                if bcount >= 4 && slice[0..4] == *b"wvpk" {
                    dprintln!("WavPack output_callback: Writing WavPack header block");
                }
            } else if bcount > 1000 {
                dprintln!(
                    "WavPack output_callback: Large write of {} bytes (total: {})",
                    bcount,
                    encoder.output_buffer.len()
                );
            }

            bcount
        }

        self.context = unsafe {
            WavpackOpenFileOutput(
                block_output_callback,
                self as *mut Self as *mut c_void,
                ptr::null_mut(),
            )
        };

        if self.context.is_null() {
            return Err(anyhow!("Failed to create WavPack encoder context"));
        }

        // Set file information
        let extension = CString::new("wv").unwrap();
        unsafe {
            WavpackSetFileInformation(
                self.context,
                extension.as_ptr() as *mut c_char,
                WP_FORMAT_WAV,
            );
        }

        Ok(())
    }


    /// Encode an AudioBuffer to WavPack format
    pub fn encode(
        &mut self,
        buffer: &AudioBuffer,
        total_samples: u64,
        metadata: &Option<&Metadata>,
    ) -> R<Vec<u8>> {
        if self.context.is_null() {
            return Err(anyhow!("Encoder not initialized"));
        }

        // Set total samples in configuration
        let result = unsafe {
            WavpackSetConfiguration64(
                self.context,
                &mut self.config,
                total_samples as int64_t,
                ptr::null(),
            )
        };

        if result == 0 {
            return Err(anyhow!("Failed to set WavPack configuration"));
        }

        // Initialize packing
        let init_result = unsafe { WavpackPackInit(self.context) };
        if init_result == 0 {
            return Err(anyhow!("Failed to initialize WavPack packing"));
        }

        // Metadata will be added after pack init but before encoding samples
        // This is handled by the codec's add_metadata_to_encoder method

        // Convert and interleave audio data
        let samples_per_channel = buffer.data[0].len();
        let channels = buffer.channels as usize;
        let mut interleaved_samples = vec![0i32; samples_per_channel * channels];

        self.interleave_and_convert_samples(buffer, &mut interleaved_samples)?;

        // Pack samples in chunks
        const CHUNK_SIZE: usize = 4096;
        let mut sample_pos = 0;

        while sample_pos < samples_per_channel {
            let samples_to_pack = std::cmp::min(CHUNK_SIZE, samples_per_channel - sample_pos);
            let start_idx = sample_pos * channels;
            let end_idx = (sample_pos + samples_to_pack) * channels;

            let pack_result = unsafe {
                WavpackPackSamples(
                    self.context,
                    interleaved_samples[start_idx..end_idx].as_mut_ptr(),
                    samples_to_pack as uint32_t,
                )
            };

            if pack_result == 0 {
                return Err(anyhow!("Failed to pack WavPack samples"));
            }

            sample_pos += samples_to_pack;
        }

        // Flush remaining samples
        let flush_result = unsafe { WavpackFlushSamples(self.context) };
        if flush_result == 0 {
            return Err(anyhow!("Failed to flush WavPack samples"));
        }

        // CRITICAL: Write metadata tags AFTER all audio data has been encoded
        // This ensures the WavPack header comes first, then audio data, then metadata
        if metadata.is_some() {
            dprintln!("WavPack encode: Writing metadata tags to output stream...");
            let write_result = unsafe { WavpackWriteTag(self.context) };
            if write_result == 0 {
                dprintln!("WavPack encode: WARNING - WavpackWriteTag() failed");
            } else {
                dprintln!("WavPack encode: WavpackWriteTag() successful");
            }
        }

        dprintln!(
            "WavPack encode: After flush, output buffer has {} bytes",
            self.output_buffer.len()
        );

        // Verify metadata is still in the context after encoding
        let final_text_tags = unsafe { WavpackGetNumTagItems(self.context) };
        let final_binary_tags = unsafe { WavpackGetNumBinaryTagItems(self.context) };
        dprintln!(
            "WavPack encode: Final verification - context has {} text tags and {} binary tags",
            final_text_tags,
            final_binary_tags
        );

        Ok(std::mem::take(&mut self.output_buffer))
    }

    /// Convert f32 samples to i32 and interleave by channels
    fn interleave_and_convert_samples(&self, buffer: &AudioBuffer, output: &mut [i32]) -> R<()> {
        let samples_per_channel = buffer.data[0].len();
        let channels = buffer.channels as usize;
        let bits_per_sample = self.config.bits_per_sample;
        let is_float = (self.config.flags & CONFIG_EXTRA_MODE) != 0;

        for i in 0..samples_per_channel {
            for ch in 0..channels {
                let sample_f32 = buffer.data[ch][i];
                let sample_i32 = match (bits_per_sample, is_float) {
                    (8, false) => {
                        let unsigned_val = ((sample_f32 + 1.0) * 127.5) as u8;
                        (unsigned_val as i8) as i32
                    }
                    (16, false) => (sample_f32 * 32768.0) as i32,
                    (24, false) => (sample_f32 * 8388608.0) as i32,
                    (32, false) => (sample_f32 * 2147483648.0) as i32,
                    (32, true) => sample_f32.to_bits() as i32,
                    _ => (sample_f32 * 32768.0) as i32, // Default to 16-bit
                };

                let output_idx = i * channels + ch;
                if output_idx < output.len() {
                    output[output_idx] = sample_i32;
                }
            }
        }

        Ok(())
    }
}

impl Drop for WavpackEncoder {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe {
                WavpackCloseFile(self.context);
            }
        }

        if let Some(ref temp_file) = self.temp_file {
            let _ = std::fs::remove_file(temp_file);
        }
    }
}

/// WavPack codec implementation
pub struct WvCodec;

impl Codec for WvCodec {
    fn file_extension(&self) -> &'static str {
        "wv"
    }

    fn validate_file_format(&self, data: &[u8]) -> R<()> {
        // WavPack files start with "wvpk" signature
        if data.len() < 4 {
            return Err(anyhow!("File too small to be a valid WavPack file"));
        }

        if &data[0..4] != b"wvpk" {
            return Err(anyhow!("Invalid WavPack file: Missing 'wvpk' signature"));
        }

        Ok(())
    }
    fn get_file_info(&self, file_path: &str) -> R<FileInfo> {
        use memmap2::MmapOptions;
        use std::fs;

        let file = fs::File::open(file_path)?;
        let file_size = file.metadata()?.len() as usize;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        self.validate_file_format(&mapped_file)?;

        // Use WavpackDecoder to extract file information
        let decoder = WavpackDecoder::new(&mapped_file)?;

        let sample_rate = decoder.sample_rate();
        let channels = decoder.channels();
        let total_samples = decoder.total_samples();

        // Extract description from metadata if available
        let mut description = String::new();
        if let Ok(metadata) = self.parse_metadata(&mapped_file) {
            // Look for description-like fields in the metadata
            for (key, value) in metadata.get_all_fields() {
                if (key.to_lowercase().contains("description")
                    || key.to_lowercase().contains("comment"))
                    && !value.trim().is_empty()
                {
                    description = value.trim().to_string();
                    break;
                }
            }
        }

        // WavPack doesn't store bit depth in the same way as other formats
        // It's a lossless format that can contain various bit depths
        // We'll try to determine it from the format flags or default to 16-bit
        let bit_depth = match decoder.bits_per_sample() {
            0 => 16, // Default fallback
            bits => bits,
        };

        // Calculate duration
        let duration_seconds = if sample_rate > 0 && total_samples > 0 {
            total_samples as f64 / sample_rate as f64
        } else {
            0.0
        };

        let duration = if duration_seconds >= 3600.0 {
            format!(
                "{:.0}:{:02.0}:{:02.0}",
                duration_seconds / 3600.0,
                (duration_seconds % 3600.0) / 60.0,
                duration_seconds % 60.0
            )
        } else {
            format!(
                "{:.0}:{:02.0}",
                duration_seconds / 60.0,
                duration_seconds % 60.0
            )
        };

        Ok(FileInfo {
            path: file_path.to_string(),
            size: file_size,
            sample_rate: sample_rate as u16,
            channels,
            bit_depth: bit_depth as u16,
            duration,
            description,
        })
    }

    fn decode(&self, input: &[u8]) -> R<AudioBuffer> {
        self.validate_file_format(input)?;

        // Use the WavpackDecoder to handle the decoding
        let mut decoder = WavpackDecoder::new(input)?;
        decoder.decode()
    }

    fn encode(&self, buffer: &Option<AudioBuffer>) -> R<Vec<u8>> {
        let Some(buffer) = buffer else {
            return Err(anyhow!("Cannot encode None AudioBuffer"));
        };
        // Validate input buffer
        if buffer.data.is_empty() || buffer.data[0].is_empty() {
            return Err(anyhow!("Empty audio buffer provided"));
        }

        if buffer.channels as usize != buffer.data.len() {
            return Err(anyhow!("Mismatch between channel count and data channels"));
        }

        // Determine encoding parameters
        let sample_rate = buffer.sample_rate;
        let channels = buffer.channels;
        let bits_per_sample = match buffer.format {
            SampleFormat::U8 => 8,
            SampleFormat::I16 => 16,
            SampleFormat::I24 => 24,
            SampleFormat::I32 => 32,
            SampleFormat::F32 => 32,
        };
        let is_float = buffer.format == SampleFormat::F32;
        let lossless = true; // Default to lossless encoding
        let total_samples = buffer.data[0].len() as u64;

        // Create and initialize encoder
        let mut encoder =
            WavpackEncoder::new(sample_rate, channels, bits_per_sample, is_float, lossless)?;

        encoder.init()?;

        // Encode the audio buffer (no metadata for plain encode)
        encoder.encode(buffer, total_samples, &None)
    }


    fn parse_metadata(&self, input: &[u8]) -> R<Metadata> {
        let mut metadata = Metadata::new();
        let decoder = WavpackDecoder::new(input)?;

        // Extract text tags
        let num_tags = unsafe { WavpackGetNumTagItems(decoder.context) };
        
        for i in 0..num_tags {
            let mut item_buffer = vec![0u8; 256];
            let result = unsafe {
                WavpackGetTagItemIndexed(
                    decoder.context,
                    i,
                    item_buffer.as_mut_ptr() as *mut c_char,
                    item_buffer.len() as c_int,
                )
            };

            if result > 0 {
                // Get the item name
                let item_end = item_buffer
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(item_buffer.len());
                let item_name = String::from_utf8_lossy(&item_buffer[..item_end]).to_string();

                // Get the item value
                let mut value_buffer = vec![0u8; 1024];
                let value_len = unsafe {
                    WavpackGetTagItem(
                        decoder.context,
                        item_name.as_ptr() as *const c_char,
                        value_buffer.as_mut_ptr() as *mut c_char,
                        value_buffer.len() as c_int,
                    )
                };

                if value_len > 0 {
                    let value_end = value_buffer
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(value_buffer.len());
                    let value = String::from_utf8_lossy(&value_buffer[..value_end]).to_string();
                    if !item_name.is_empty() && !value.is_empty() {
                        // Special handling for iXML content
                        if item_name.to_uppercase() == "IXML" {
                            metadata.parse_ixml(&value)?;
                        } else {
                            // Map common WavPack tag names to standard names
                            let standard_key = self.normalize_wavpack_key(&item_name);
                            metadata.set_field(&standard_key, &value)?;
                        }
                    }
                }
            }
        }

        // Extract binary tags (like album art)
        let num_binary_tags = unsafe { WavpackGetNumBinaryTagItems(decoder.context) };
        
        for i in 0..num_binary_tags {
            let mut item_buffer = vec![0u8; 256];
            let result = unsafe {
                WavpackGetBinaryTagItemIndexed(
                    decoder.context,
                    i,
                    item_buffer.as_mut_ptr() as *mut c_char,
                    item_buffer.len() as c_int,
                )
            };

            if result > 0 {
                let item_end = item_buffer
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(item_buffer.len());
                let item_name = String::from_utf8_lossy(&item_buffer[..item_end]).to_string();

                // Get binary data - start with reasonable size and grow if needed
                let mut data_buffer = vec![0u8; 2 * 1024 * 1024]; // 2MB initial
                let data_len = unsafe {
                    WavpackGetBinaryTagItem(
                        decoder.context,
                        item_name.as_ptr() as *const c_char,
                        data_buffer.as_mut_ptr() as *mut c_char,
                        data_buffer.len() as c_int,
                    )
                };

                if data_len > 0 {
                    data_buffer.truncate(data_len as usize);

                    // Check if this looks like image data
                    if item_name.to_lowercase().contains("cover")
                        || item_name.to_lowercase().contains("art")
                        || item_name.to_lowercase().contains("picture")
                        || item_name.to_lowercase().contains("apic")
                    {
                        let mime_type = Self::detect_image_mime_type(&data_buffer);
                        let image = ImageChunk {
                            mime_type,
                            description: item_name,
                            data: data_buffer,
                        };
                        metadata.add_image(image);
                    }
                }
            }
        }

        // Add wrapper information if available
        let wrapper_bytes = unsafe { WavpackGetWrapperBytes(decoder.context) };
        if wrapper_bytes > 0 {
            let wrapper_data = unsafe { WavpackGetWrapperData(decoder.context) };
            if !wrapper_data.is_null() {
                let _wrapper_slice =
                    unsafe { std::slice::from_raw_parts(wrapper_data, wrapper_bytes as usize) };
                // Store wrapper data as a field for now
                metadata.set_field("WavpackWrapper", &format!("{} bytes", wrapper_bytes))?;
            }
        }

        Ok(metadata)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()> {
        // For WavPack, we need to decode, add metadata, and re-encode
        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };
        
        // First, decode the WavPack file to get the audio data
        let audio_buffer = self.decode(&mapped_file)?;
        
        // Create a new encoder with the same parameters
        let sample_rate = audio_buffer.sample_rate;
        let channels = audio_buffer.channels;
        let bits_per_sample = match audio_buffer.format {
            SampleFormat::U8 => 8,
            SampleFormat::I16 => 16,
            SampleFormat::I24 => 24,
            SampleFormat::I32 => 32,
            SampleFormat::F32 => 32,
        };
        let is_float = audio_buffer.format == SampleFormat::F32;
        let lossless = true;
        let total_samples = audio_buffer.data[0].len() as u64;
        
        let mut encoder =
            WavpackEncoder::new(sample_rate, channels, bits_per_sample, is_float, lossless)?;
        
        encoder.init()?;
        
        // Add metadata to the encoder context before encoding
        self.add_metadata_to_encoder(&mut encoder, metadata)?;
        
        // Encode with the metadata
        let result = encoder.encode(&audio_buffer, total_samples, &Some(metadata))?;
        
        // Write the result back to the file
        std::fs::write(file_path, result)?;
        Ok(())
    }
}

impl WvCodec {
    /// Add metadata to encoder before encoding
    fn add_metadata_to_encoder(&self, encoder: &mut WavpackEncoder, metadata: &Metadata) -> R<()> {
        if encoder.context.is_null() {
            return Err(anyhow!("Encoder not initialized"));
        }

        // Add text fields from the hashmap
        for (key, value) in metadata.get_all_fields().iter() {
            let wavpack_key = self.map_to_wavpack_key(key);
            let c_key = CString::new(wavpack_key.as_str())
                .map_err(|_| anyhow!("Invalid metadata key"))?;
            let c_value = CString::new(value.as_str())
                .map_err(|_| anyhow!("Invalid metadata value"))?;

            let result = unsafe {
                WavpackAppendTagItem(
                    encoder.context,
                    c_key.as_ptr(),
                    c_value.as_ptr(),
                    value.len() as c_int,
                )
            };
            
            if result != 1 {
                dprintln!("Warning: Failed to add text tag '{}'", key);
            }
        }

        // Create and add iXML from all metadata fields
        if !metadata.get_all_fields().is_empty() {
            let ixml_content = self.create_ixml_from_metadata(metadata)?;
            let c_key = CString::new("iXML")
                .map_err(|_| anyhow!("Invalid metadata key"))?;
            let c_value = CString::new(ixml_content.as_str())
                .map_err(|_| anyhow!("Invalid metadata value"))?;

            let result = unsafe {
                WavpackAppendTagItem(
                    encoder.context,
                    c_key.as_ptr(),
                    c_value.as_ptr(),
                    ixml_content.len() as c_int,
                )
            };
            
            if result != 1 {
                dprintln!("Warning: Failed to add iXML tag");
            }
        }

        // Add image data as binary tags
        for image in metadata.get_images() {
            let item_name = if image.description().to_lowercase().contains("cover")
                || image.description().to_lowercase().contains("art")
            {
                "Cover Art"
            } else {
                "Picture"
            };

            let c_item = CString::new(item_name)
                .map_err(|_| anyhow!("Invalid item name"))?;

            let result = unsafe {
                WavpackAppendBinaryTagItem(
                    encoder.context,
                    c_item.as_ptr(),
                    image.data().as_ptr() as *const c_char,
                    image.data().len() as c_int,
                )
            };
            
            if result != 1 {
                dprintln!("Warning: Failed to add picture '{}'", image.description());
            }
        }

        Ok(())
    }

    /// Normalize WavPack tag names to standard metadata field names
    fn normalize_wavpack_key(&self, key: &str) -> String {
        match key.to_uppercase().as_str() {
            "TITLE" => "Title".to_string(),
            "ARTIST" => "Artist".to_string(),
            "ALBUM" => "Album".to_string(),
            "DATE" | "YEAR" => "Year".to_string(),
            "GENRE" => "Genre".to_string(),
            "TRACKNUMBER" | "TRACK" => "Track".to_string(),
            "ALBUMARTIST" => "AlbumArtist".to_string(),
            "COMPOSER" => "Composer".to_string(),
            "CONDUCTOR" => "Conductor".to_string(),
            "COMMENT" => "Comment".to_string(),
            "DESCRIPTION" => "Description".to_string(),
            "DISCNUMBER" | "DISC" => "DiscNumber".to_string(),
            "ORGANIZATION" | "PUBLISHER" => "Publisher".to_string(),
            "COPYRIGHT" => "Copyright".to_string(),
            "ISRC" => "ISRC".to_string(),
            "ENCODER" => "EncodingSettings".to_string(),
            "LANGUAGE" => "Language".to_string(),
            "PERFORMER" => "Performer".to_string(),
            _ => {
                // Preserve WAV-specific prefixed fields for cross-format compatibility
                if key.starts_with("USER_") || key.starts_with("BEXT_") || 
                   key.starts_with("ASWG_") || key.starts_with("STEINBERG_") {
                    key.to_string()
                } else {
                    key.to_string()
                }
            }
        }
    }

    /// Map standard metadata field names to WavPack tag names
    fn map_to_wavpack_key(&self, key: &str) -> String {
        match key {
            "Title" => "TITLE".to_string(),
            "Artist" => "ARTIST".to_string(),
            "Album" => "ALBUM".to_string(),
            "Year" => "DATE".to_string(),
            "Genre" => "GENRE".to_string(),
            "Track" => "TRACKNUMBER".to_string(),
            "AlbumArtist" => "ALBUMARTIST".to_string(),
            "Composer" => "COMPOSER".to_string(),
            "Conductor" => "CONDUCTOR".to_string(),
            "Comment" => "COMMENT".to_string(),
            "Description" => "DESCRIPTION".to_string(),
            "DiscNumber" => "DISCNUMBER".to_string(),
            "Publisher" => "ORGANIZATION".to_string(),
            "Copyright" => "COPYRIGHT".to_string(),
            "ISRC" => "ISRC".to_string(),
            "EncodingSettings" => "ENCODER".to_string(),
            "Language" => "LANGUAGE".to_string(),
            "Performer" => "PERFORMER".to_string(),
            // For any other keys, use uppercase (WavPack convention)
            // But preserve WAV-specific prefixed fields for cross-format compatibility
            _ => {
                if key.starts_with("USER_") || key.starts_with("BEXT_") || 
                   key.starts_with("ASWG_") || key.starts_with("STEINBERG_") {
                    key.to_string()
                } else {
                    key.to_uppercase()
                }
            }
        }
    }

    /// Create iXML from all metadata fields
    fn create_ixml_from_metadata(&self, metadata: &Metadata) -> R<String> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<WAVPACKXML>\n");
        xml.push_str(&crate::ixml::create_ixml_from_metadata(metadata)?);
        xml.push_str("</WAVPACKXML>\n");
        Ok(xml)
    }

    /// Detect MIME type from image data
    #[allow(dead_code)]
    fn detect_image_mime_type(data: &[u8]) -> String {
        if data.len() < 8 {
            return "application/octet-stream".to_string();
        }

        if data.starts_with(b"\xFF\xD8\xFF") {
            "image/jpeg".to_string()
        } else if data.starts_with(b"\x89PNG\r\n\x1A\n") {
            "image/png".to_string()
        } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            "image/gif".to_string()
        } else if data.starts_with(b"RIFF") && data[8..12] == *b"WEBP" {
            "image/webp".to_string()
        } else if data.starts_with(b"\x00\x00\x01\x00") {
            "image/x-icon".to_string()
        } else {
            "application/octet-stream".to_string()
        }
    }
}
