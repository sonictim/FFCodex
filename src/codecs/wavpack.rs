//! WavPack codec implementation for FFCodex
//!
//! This module provides complete WavPack (.wv) format support including:
//! - Lossless and hybrid lossy compression
//! - Multi-channel audio support
//! - Comprehensive metadata handling
//! - High-quality encoding and decoding

use crate::bindings::wavpack_bindings::*;
use crate::prelude::*;
use memmap2::MmapOptions;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::io::Cursor;

/// Source format types for wrapper generation
#[derive(Debug, Clone, Copy, PartialEq)]
enum SourceFormat {
    WAV,
    FLAC,
    AIFF,
    Unknown,
}

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
                // dprintln!("WavPack output_callback: First write of {} bytes", bcount);
                // Check if this looks like a header (WavPack files start with "wvpk")
                if bcount >= 4 && slice[0..4] == *b"wvpk" {
                    // dprintln!("WavPack output_callback: Writing WavPack header block");
                }
            } else if bcount > 1000 {
                // dprintln!(
                //     "WavPack output_callback: Large write of {} bytes (total: {})",
                //     bcount,
                //     encoder.output_buffer.len()
                // );
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
    fn as_str(&self) -> &'static str {
        "WAVPACK"
    }
    fn file_extension(&self) -> &'static str {
        "wv"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
        self.encode_with_metadata(buffer, &None)
    }

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata> {
        let mut metadata = Metadata::new();
        let decoder = WavpackDecoder::new(input)?;

        // Extract text tags
        let num_tags = unsafe { WavpackGetNumTagItems(decoder.context) };
        dprintln!("WavPack parse_metadata: Found {} text tags", num_tags);

        for i in 0..num_tags {
            let mut item_buffer = vec![0u8; 256];
            let result = unsafe {
                WavpackGetTagItemIndexed(
                    decoder.context,
                    i as c_int,
                    item_buffer.as_mut_ptr() as *mut c_char,
                    item_buffer.len() as c_int,
                )
            };

            if result > 0 {
                let item_name = String::from_utf8_lossy(&item_buffer[..result as usize]);
                let mut value_buffer = vec![0u8; 1024];
                let value_result = unsafe {
                    WavpackGetTagItem(
                        decoder.context,
                        item_name.as_ptr() as *const c_char,
                        value_buffer.as_mut_ptr() as *mut c_char,
                        value_buffer.len() as c_int,
                    )
                };

                if value_result > 0 {
                    let value = String::from_utf8_lossy(&value_buffer[..value_result as usize]);
                    dprintln!(
                        "WavPack parse_metadata: Found tag '{}' = '{}'",
                        item_name,
                        value
                    );
                    // Special handling for iXML content
                    if item_name.to_uppercase() == "IXML" {
                        metadata.parse_ixml(&value)?;
                    } else {
                        // Map common WavPack tag names to standard names with TAG_ prefix
                        let standard_key = self.normalize_wavpack_key(&item_name);
                        let prefixed_key = format!("TAG_{}", standard_key);
                        metadata.set_field(&prefixed_key, &value)?;
                    }
                }
            }
        }

        // Extract binary tags
        let num_binary_tags = unsafe { WavpackGetNumBinaryTagItems(decoder.context) };
        dprintln!(
            "WavPack parse_metadata: Found {} binary tags",
            num_binary_tags
        );

        for i in 0..num_binary_tags {
            let mut item_buffer = vec![0u8; 256];
            let result = unsafe {
                WavpackGetBinaryTagItemIndexed(
                    decoder.context,
                    i as c_int,
                    item_buffer.as_mut_ptr() as *mut c_char,
                    item_buffer.len() as c_int,
                )
            };

            if result > 0 {
                let item_name = String::from_utf8_lossy(&item_buffer[..result as usize]);
                let binary_size = unsafe {
                    WavpackGetBinaryTagItem(
                        decoder.context,
                        item_name.as_ptr() as *const c_char,
                        ptr::null_mut(),
                        0,
                    )
                };

                if binary_size > 0 {
                    let mut binary_data = vec![0u8; binary_size as usize];
                    let binary_result = unsafe {
                        WavpackGetBinaryTagItem(
                            decoder.context,
                            item_name.as_ptr() as *const c_char,
                            binary_data.as_mut_ptr() as *mut c_char,
                            binary_size,
                        )
                    };

                    if binary_result > 0 {
                        dprintln!(
                            "WavPack parse_metadata: Found binary tag '{}' ({} bytes)",
                            item_name,
                            binary_data.len()
                        );

                        match item_name.as_ref() {
                            "Cover Art (Front)" | "APIC" => {
                                // Image data
                                let mime_type = detect_image_mime_type(&binary_data);
                                let image_chunk = ImageChunk::new(
                                    mime_type,
                                    "Cover Art".to_string(),
                                    binary_data,
                                );
                                metadata.add_image(image_chunk);
                            }
                            _ => {
                                // For other binary tags, try to parse as text if possible
                                if let Ok(text_data) = String::from_utf8(binary_data) {
                                    if !text_data.trim().is_empty() {
                                        metadata.set_field(&item_name, &text_data)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Extract format information for metadata completion
        let channels = unsafe { WavpackGetNumChannels(decoder.context) };
        let sample_rate = unsafe { WavpackGetSampleRate(decoder.context) };
        let bits_per_sample = unsafe { WavpackGetBitsPerSample(decoder.context) };

        metadata.channels = channels as u16;
        metadata.sample_rate = sample_rate;
        metadata.bit_depth = bits_per_sample as u16;
        metadata.format_tag = 0xFFFF; // WavPack's own format tag

        // Extract wrapper data if present
        if unsafe { WavpackGetWrapperBytes(decoder.context) } > 0 {
            let wrapper_size = unsafe { WavpackGetWrapperBytes(decoder.context) };
            let wrapper_data = unsafe { 
                let ptr = WavpackGetWrapperData(decoder.context);
                if !ptr.is_null() {
                    std::slice::from_raw_parts(ptr, wrapper_size as usize).to_vec()
                } else {
                    Vec::new()
                }
            };

            if !wrapper_data.is_empty() {
                dprintln!(
                    "WavPack parse_metadata: Found wrapper data ({} bytes)",
                    wrapper_data.len()
                );
                self.parse_wrapper_metadata(&mut metadata, &wrapper_data)?;
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

        // Verify metadata was added to context
        let text_tags = unsafe { WavpackGetNumTagItems(encoder.context) };
        let binary_tags = unsafe { WavpackGetNumBinaryTagItems(encoder.context) };
        dprintln!(
            "WavPack embed_metadata_to_file: After adding metadata - context has {} text tags and {} binary tags",
            text_tags,
            binary_tags
        );

        // Encode with the metadata
        let result = encoder.encode(&audio_buffer, total_samples, &Some(metadata))?;

        // Write the result back to the file
        std::fs::write(file_path, result)?;
        Ok(())
    }
}

impl WvCodec {
    /// Encode with optional metadata - avoids double encoding for WavPack
    pub fn encode_with_metadata(&self, buffer: &Option<AudioBuffer>, metadata: &Option<&Metadata>) -> R<Vec<u8>> {
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

        // Add metadata to encoder context if provided
        if let Some(metadata) = metadata {
            self.add_metadata_to_encoder(&mut encoder, metadata)?;
        }

        // Encode the audio buffer with metadata
        encoder.encode(buffer, total_samples, metadata)
    }

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata> {
        let mut metadata = Metadata::new();
        let decoder = WavpackDecoder::new(input)?;

        // Extract text tags
        let num_tags = unsafe { WavpackGetNumTagItems(decoder.context) };
        dprintln!("WavPack parse_metadata: Found {} text tags", num_tags);

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
                        dprintln!(
                            "WavPack parse_metadata: Found tag '{}' = '{}'",
                            item_name,
                            value
                        );
                        // Special handling for iXML content
                        if item_name.to_uppercase() == "IXML" {
                            metadata.parse_ixml(&value)?;
                        } else {
                            // Map common WavPack tag names to standard names with TAG_ prefix
                            let standard_key = self.normalize_wavpack_key(&item_name);
                            let prefixed_key = format!("TAG_{}", standard_key);
                            metadata.set_field(&prefixed_key, &value)?;
                        }
                    }
                }
            }
        }

        // Extract binary tags (like album art)
        let num_binary_tags = unsafe { WavpackGetNumBinaryTagItems(decoder.context) };
        dprintln!(
            "WavPack parse_metadata: Found {} binary tags",
            num_binary_tags
        );

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
                        let mime_type = detect_image_mime_type(&data_buffer);
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

        // Parse wrapper data if available - this contains embedded format metadata
        let wrapper_bytes = unsafe { WavpackGetWrapperBytes(decoder.context) };
        if wrapper_bytes > 0 {
            let wrapper_data = unsafe { WavpackGetWrapperData(decoder.context) };
            if !wrapper_data.is_null() {
                let wrapper_slice =
                    unsafe { std::slice::from_raw_parts(wrapper_data, wrapper_bytes as usize) };
                
                dprintln!(
                    "WavPack parse_metadata: Found wrapper data ({} bytes), parsing embedded metadata...",
                    wrapper_bytes
                );
                
                // Store the raw wrapper data for perfect re-embedding (as hex)
                let wrapper_hex = wrapper_slice.iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                metadata.set_field("WAVPACK_WRAPPER_DATA", &wrapper_hex)?;
                metadata.set_field("WAVPACK_WRAPPER_SIZE", &wrapper_bytes.to_string())?;
                
                // Parse the wrapper data as the original format's metadata
                if let Err(e) = self.parse_wrapper_metadata(&mut metadata, wrapper_slice) {
                    dprintln!("WavPack parse_metadata: Warning - failed to parse wrapper metadata: {}", e);
                }
            }
        }

        // Extract additional WavPack-specific metadata
        self.extract_wavpack_technical_metadata(&mut metadata, &decoder)?;

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

        // Verify metadata was added to context
        let text_tags = unsafe { WavpackGetNumTagItems(encoder.context) };
        let binary_tags = unsafe { WavpackGetNumBinaryTagItems(encoder.context) };
        dprintln!(
            "WavPack embed_metadata_to_file: After adding metadata - context has {} text tags and {} binary tags",
            text_tags,
            binary_tags
        );

        // Encode with the metadata
        let result = encoder.encode(&audio_buffer, total_samples, &Some(metadata))?;

        // Write the result back to the file
        std::fs::write(file_path, result)?;
        Ok(())
    }
}

impl WvCodec {
    /// Parse embedded metadata from WavPack wrapper data
    fn parse_wrapper_metadata(&self, metadata: &mut Metadata, wrapper_data: &[u8]) -> R<()> {
        if wrapper_data.len() < 8 {
            return Ok(());
        }

        // WavPack wrapper data typically contains the original format's chunks
        // Try to identify the format and parse accordingly
        
        // Check for WAV format wrapper (RIFF/WAVE)
        if wrapper_data.len() >= 12 && 
           &wrapper_data[0..4] == b"RIFF" && 
           &wrapper_data[8..12] == b"WAVE" {
            dprintln!("WavPack parse_wrapper_metadata: Found WAV wrapper data");
            return self.parse_wav_wrapper_chunks(metadata, wrapper_data);
        }
        
        // Check for AIFF format wrapper (FORM/AIFF)
        if wrapper_data.len() >= 12 && 
           &wrapper_data[0..4] == b"FORM" && 
           &wrapper_data[8..12] == b"AIFF" {
            dprintln!("WavPack parse_wrapper_metadata: Found AIFF wrapper data");
            return self.parse_aiff_wrapper_chunks(metadata, wrapper_data);
        }
        
        // Try to parse as generic chunks
        dprintln!("WavPack parse_wrapper_metadata: Parsing as generic chunk data");
        self.parse_generic_wrapper_chunks(metadata, wrapper_data)
    }

    /// Parse WAV format wrapper chunks
    fn parse_wav_wrapper_chunks(&self, metadata: &mut Metadata, data: &[u8]) -> R<()> {
        let mut cursor = Cursor::new(data);
        cursor.set_position(12); // Skip RIFF/WAVE header

        while cursor.position() + 8 <= data.len() as u64 {
            let pos = cursor.position() as usize;
            let chunk_id = &data[pos..pos + 4];
            let chunk_size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
            
            if pos + 8 + chunk_size as usize > data.len() {
                break;
            }
            
            let chunk_data = &data[pos + 8..pos + 8 + chunk_size as usize];
            
            match chunk_id {
                b"bext" => {
                    dprintln!("WavPack wrapper: Found BWF bext chunk");
                    metadata.parse_bext(chunk_data)?;
                }
                b"iXML" => {
                    dprintln!("WavPack wrapper: Found iXML chunk");
                    let xml_str = String::from_utf8_lossy(chunk_data);
                    metadata.parse_ixml(&xml_str)?;
                }
                b"ID3 " | b"id3 " => {
                    dprintln!("WavPack wrapper: Found ID3 chunk");
                    metadata.parse_id3(chunk_data)?;
                }
                b"LIST" => {
                    if chunk_size >= 4 && &chunk_data[0..4] == b"INFO" {
                        dprintln!("WavPack wrapper: Found LIST INFO chunk");
                        self.parse_list_info_chunk(metadata, &chunk_data[4..])?;
                    }
                }
                _ => {
                    // Store unknown chunks for debugging
                    let chunk_name = String::from_utf8_lossy(chunk_id);
                    if chunk_name.chars().all(|c| c.is_ascii_graphic()) {
                        dprintln!("WavPack wrapper: Found chunk '{}' ({} bytes)", chunk_name, chunk_size);
                    }
                }
            }
            
            cursor.set_position(pos as u64 + 8 + chunk_size as u64);
            if chunk_size % 2 == 1 {
                cursor.set_position(cursor.position() + 1); // Padding
            }
        }
        
        Ok(())
    }

    /// Parse AIFF format wrapper chunks  
    fn parse_aiff_wrapper_chunks(&self, metadata: &mut Metadata, data: &[u8]) -> R<()> {
        let mut cursor = Cursor::new(data);
        cursor.set_position(12); // Skip FORM/AIFF header

        while cursor.position() + 8 <= data.len() as u64 {
            let pos = cursor.position() as usize;
            let chunk_id = &data[pos..pos + 4];
            let chunk_size = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
            
            if pos + 8 + chunk_size as usize > data.len() {
                break;
            }
            
            let chunk_data = &data[pos + 8..pos + 8 + chunk_size as usize];
            
            match chunk_id {
                b"iXML" => {
                    dprintln!("WavPack wrapper: Found AIFF iXML chunk");
                    let xml_str = String::from_utf8_lossy(chunk_data);
                    metadata.parse_ixml(&xml_str)?;
                }
                b"ID3 " | b"id3 " => {
                    dprintln!("WavPack wrapper: Found AIFF ID3 chunk");
                    metadata.parse_id3(chunk_data)?;
                }
                _ => {
                    let chunk_name = String::from_utf8_lossy(chunk_id);
                    if chunk_name.chars().all(|c| c.is_ascii_graphic()) {
                        dprintln!("WavPack wrapper: Found AIFF chunk '{}' ({} bytes)", chunk_name, chunk_size);
                    }
                }
            }
            
            cursor.set_position(pos as u64 + 8 + chunk_size as u64);
            if chunk_size % 2 == 1 {
                cursor.set_position(cursor.position() + 1); // Padding
            }
        }
        
        Ok(())
    }

    /// Parse generic wrapper chunks
    fn parse_generic_wrapper_chunks(&self, metadata: &mut Metadata, data: &[u8]) -> R<()> {
        // Try to find iXML or ID3 data within the wrapper
        let data_str = String::from_utf8_lossy(data);
        
        // Look for iXML content
        if let Some(start) = data_str.find("<BWFXML>") {
            if let Some(end) = data_str.find("</BWFXML>") {
                let ixml_content = &data_str[start..end + 9];
                dprintln!("WavPack wrapper: Found embedded iXML content");
                metadata.parse_ixml(ixml_content)?;
            }
        }
        
        Ok(())
    }

    /// Extract WavPack-specific technical metadata
    fn extract_wavpack_technical_metadata(&self, metadata: &mut Metadata, decoder: &WavpackDecoder) -> R<()> {
        // Get file format information
        let mode = unsafe { WavpackGetMode(decoder.context) };
        metadata.set_field("WAVPACK_MODE", &format!("{:#x}", mode))?;
        
        // Check various format flags
        if (mode & MODE_LOSSLESS) != 0 {
            metadata.set_field("WAVPACK_LOSSLESS", "true")?;
        }
        if (mode & MODE_HYBRID) != 0 {
            metadata.set_field("WAVPACK_HYBRID", "true")?;
        }
        if (mode & MODE_FLOAT) != 0 {
            metadata.set_field("WAVPACK_FLOAT", "true")?;
        }
        if (mode & MODE_HIGH) != 0 {
            metadata.set_field("WAVPACK_HIGH_QUALITY", "true")?;
        }
        if (mode & MODE_FAST) != 0 {
            metadata.set_field("WAVPACK_FAST_MODE", "true")?;
        }
        
        // Get version information
        let version = unsafe { WavpackGetVersion(decoder.context) };
        metadata.set_field("WAVPACK_VERSION", &version.to_string())?;
        
        // Get file size information
        let file_size = unsafe { WavpackGetFileSize64(decoder.context) };
        if file_size > 0 {
            metadata.set_field("WAVPACK_FILE_SIZE", &file_size.to_string())?;
        }
        
        // Get ratio information (compression ratio)
        let ratio = unsafe { WavpackGetRatio(decoder.context) };
        if ratio > 0.0 {
            metadata.set_field("WAVPACK_COMPRESSION_RATIO", &format!("{:.3}", ratio))?;
        }
        
        // Get average bitrate
        let avg_bitrate = unsafe { WavpackGetAverageBitrate(decoder.context, 0) };
        if avg_bitrate > 0.0 {
            metadata.set_field("WAVPACK_AVERAGE_BITRATE", &avg_bitrate.to_string())?;
        }
        
        // Get instantaneous bitrate at different positions
        let instant_bitrate = unsafe { WavpackGetInstantBitrate(decoder.context) };
        if instant_bitrate > 0.0 {
            metadata.set_field("WAVPACK_INSTANT_BITRATE", &instant_bitrate.to_string())?;
        }
        
        // Get MD5 checksum if available
        let mut md5_sum = [0u8; 16];
        let has_md5 = unsafe { WavpackGetMD5Sum(decoder.context, md5_sum.as_mut_ptr()) };
        if has_md5 != 0 {
            let md5_hex = md5_sum.iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            metadata.set_field("WAVPACK_MD5", &md5_hex)?;
        }
        
        // Get channel mask if available
        let channel_mask = unsafe { WavpackGetChannelMask(decoder.context) };
        if channel_mask != 0 {
            metadata.set_field("WAVPACK_CHANNEL_MASK", &format!("{:#x}", channel_mask))?;
        }
        
        // Check for lossy blocks
        let num_errors = unsafe { WavpackGetNumErrors(decoder.context) };
        if num_errors > 0 {
            metadata.set_field("WAVPACK_DECODE_ERRORS", &num_errors.to_string())?;
        }
        
        dprintln!(
            "WavPack technical metadata: mode={:#x}, version={}, ratio={:.3}, bitrate={}",
            mode, version, ratio, avg_bitrate
        );
        
        Ok(())
    }

    /// Generate wrapper data from metadata when converting from non-WavPack sources
    fn generate_wrapper_data_from_metadata(&self, metadata: &Metadata) -> R<Vec<u8>> {
        // Detect source format from metadata patterns
        let source_format = self.detect_source_format(metadata);
        
        match source_format {
            SourceFormat::WAV => self.generate_wav_wrapper(metadata),
            SourceFormat::FLAC => self.generate_flac_wrapper(metadata),
            SourceFormat::AIFF => self.generate_aiff_wrapper(metadata),
            SourceFormat::Unknown => self.generate_generic_wrapper(metadata),
        }
    }

    /// Detect source format from metadata patterns
    fn detect_source_format(&self, metadata: &Metadata) -> SourceFormat {
        let fields = metadata.get_all_fields();
        
        // Check for format-specific field patterns
        if fields.keys().any(|k| k.starts_with("BEXT_") || k.starts_with("INFO_")) {
            SourceFormat::WAV
        } else if fields.keys().any(|k| k.contains("VORBIS") || k.contains("FLAC")) {
            SourceFormat::FLAC
        } else if fields.keys().any(|k| k.contains("AIFF") || k.contains("FORM")) {
            SourceFormat::AIFF
        } else {
            SourceFormat::Unknown
        }
    }

    /// Generate WAV-style wrapper data
    fn generate_wav_wrapper(&self, metadata: &Metadata) -> R<Vec<u8>> {
        let mut wrapper = Vec::new();
        
        // Create RIFF/WAVE header
        wrapper.extend_from_slice(b"RIFF");
        wrapper.extend_from_slice(&0u32.to_le_bytes()); // Size placeholder
        wrapper.extend_from_slice(b"WAVE");
        
        let mut total_size = 4; // WAVE
        
        // Add BWF bext chunk if we have BEXT fields
        if let Some(bext_chunk) = self.create_bext_chunk(metadata)? {
            wrapper.extend_from_slice(b"bext");
            wrapper.extend_from_slice(&(bext_chunk.len() as u32).to_le_bytes());
            wrapper.extend_from_slice(&bext_chunk);
            total_size += 8 + bext_chunk.len();
            
            // Add padding if needed
            if bext_chunk.len() % 2 == 1 {
                wrapper.push(0);
                total_size += 1;
            }
        }
        
        // Add iXML chunk
        let ixml_content = self.create_ixml(metadata)?;
        if !ixml_content.is_empty() {
            wrapper.extend_from_slice(b"iXML");
            wrapper.extend_from_slice(&(ixml_content.len() as u32).to_le_bytes());
            wrapper.extend_from_slice(ixml_content.as_bytes());
            total_size += 8 + ixml_content.len();
            
            // Add padding if needed
            if ixml_content.len() % 2 == 1 {
                wrapper.push(0);
                total_size += 1;
            }
        }
        
        // Add LIST INFO chunk if we have INFO fields
        if let Some(list_chunk) = self.create_list_info_chunk(metadata)? {
            wrapper.extend_from_slice(b"LIST");
            wrapper.extend_from_slice(&(list_chunk.len() as u32).to_le_bytes());
            wrapper.extend_from_slice(&list_chunk);
            total_size += 8 + list_chunk.len();
            
            // Add padding if needed
            if list_chunk.len() % 2 == 1 {
                wrapper.push(0);
                total_size += 1;
            }
        }
        
        // Update RIFF size
        let riff_size = (total_size + 4) as u32; // +4 for the size field itself
        wrapper[4..8].copy_from_slice(&riff_size.to_le_bytes());
        
        dprintln!("Generated WAV wrapper: {} bytes", wrapper.len());
        Ok(wrapper)
    }

    /// Generate FLAC-style wrapper data (minimal)
    fn generate_flac_wrapper(&self, metadata: &Metadata) -> R<Vec<u8>> {
        // For FLAC sources, create a minimal iXML-only wrapper
        let ixml_content = self.create_ixml(metadata)?;
        if ixml_content.is_empty() {
            return Ok(Vec::new());
        }
        
        dprintln!("Generated FLAC wrapper: iXML only ({} bytes)", ixml_content.len());
        Ok(ixml_content.into_bytes())
    }

    /// Generate AIFF-style wrapper data
    fn generate_aiff_wrapper(&self, metadata: &Metadata) -> R<Vec<u8>> {
        let mut wrapper = Vec::new();
        
        // Create FORM/AIFF header
        wrapper.extend_from_slice(b"FORM");
        wrapper.extend_from_slice(&0u32.to_be_bytes()); // Size placeholder
        wrapper.extend_from_slice(b"AIFF");
        
        let mut total_size = 4; // AIFF
        
        // Add iXML chunk
        let ixml_content = self.create_ixml(metadata)?;
        if !ixml_content.is_empty() {
            wrapper.extend_from_slice(b"iXML");
            wrapper.extend_from_slice(&(ixml_content.len() as u32).to_be_bytes());
            wrapper.extend_from_slice(ixml_content.as_bytes());
            total_size += 8 + ixml_content.len();
            
            // Add padding if needed
            if ixml_content.len() % 2 == 1 {
                wrapper.push(0);
                total_size += 1;
            }
        }
        
        // Update FORM size
        let form_size = (total_size + 4) as u32;
        wrapper[4..8].copy_from_slice(&form_size.to_be_bytes());
        
        dprintln!("Generated AIFF wrapper: {} bytes", wrapper.len());
        Ok(wrapper)
    }

    /// Generate generic wrapper data
    fn generate_generic_wrapper(&self, metadata: &Metadata) -> R<Vec<u8>> {
        // For unknown formats, just create iXML content
        let ixml_content = self.create_ixml(metadata)?;
        dprintln!("Generated generic wrapper: {} bytes", ixml_content.len());
        Ok(ixml_content.into_bytes())
    }

    /// Create BWF bext chunk data
    fn create_bext_chunk(&self, metadata: &Metadata) -> R<Option<Vec<u8>>> {
        let fields = metadata.get_all_fields();
        let has_bext_fields = fields.keys().any(|k| k.starts_with("BEXT_"));
        
        if !has_bext_fields {
            return Ok(None);
        }
        
        let mut bext = vec![0u8; 602]; // Standard bext size
        
        // Description (256 bytes)
        if let Some(desc) = metadata.get_field("BEXT_BWF_DESCRIPTION") {
            let desc_bytes = desc.as_bytes();
            let copy_len = std::cmp::min(desc_bytes.len(), 255);
            bext[0..copy_len].copy_from_slice(&desc_bytes[0..copy_len]);
        }
        
        // Originator (32 bytes at offset 256)
        if let Some(orig) = metadata.get_field("BEXT_ORIGINATOR") {
            let orig_bytes = orig.as_bytes();
            let copy_len = std::cmp::min(orig_bytes.len(), 31);
            bext[256..256 + copy_len].copy_from_slice(&orig_bytes[0..copy_len]);
        }
        
        // Add other BEXT fields as needed...
        
        dprintln!("Created bext chunk: {} bytes", bext.len());
        Ok(Some(bext))
    }

    /// Create LIST INFO chunk data
    fn create_list_info_chunk(&self, metadata: &Metadata) -> R<Option<Vec<u8>>> {
        let fields = metadata.get_all_fields();
        let info_fields: Vec<(&String, &String)> = fields.iter()
            .filter(|(k, _)| k.starts_with("INFO_"))
            .collect();
        
        if info_fields.is_empty() {
            return Ok(None);
        }
        
        let mut list_data = Vec::new();
        list_data.extend_from_slice(b"INFO");
        
        for (key, value) in info_fields {
            let chunk_id = &key[5..]; // Remove "INFO_" prefix
            if chunk_id.len() == 4 {
                list_data.extend_from_slice(chunk_id.as_bytes());
                list_data.extend_from_slice(&(value.len() as u32).to_le_bytes());
                list_data.extend_from_slice(value.as_bytes());
                
                // Add null terminator and padding
                if value.len() % 2 == 1 {
                    list_data.push(0);
                }
            }
        }
        
        dprintln!("Created LIST INFO chunk: {} bytes", list_data.len());
        Ok(Some(list_data))
    }

    /// Parse LIST INFO chunk from WAV wrapper
    fn parse_list_info_chunk(&self, metadata: &mut Metadata, data: &[u8]) -> R<()> {
        let mut cursor = Cursor::new(data);
        
        while cursor.position() + 8 <= data.len() as u64 {
            let pos = cursor.position() as usize;
            let chunk_id = &data[pos..pos + 4];
            let chunk_size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
            
            if pos + 8 + chunk_size as usize > data.len() {
                break;
            }
            
            let chunk_data = &data[pos + 8..pos + 8 + chunk_size as usize];
            let text_value = String::from_utf8_lossy(chunk_data)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            
            if !text_value.is_empty() {
                let chunk_name = String::from_utf8_lossy(chunk_id);
                let field_name = format!("INFO_{}", chunk_name);
                dprintln!("WavPack wrapper: Found LIST INFO '{}' = '{}'", chunk_name, text_value);
                metadata.set_field(&field_name, &text_value)?;
            }
            
            cursor.set_position(pos as u64 + 8 + chunk_size as u64);
            if chunk_size % 2 == 1 {
                cursor.set_position(cursor.position() + 1); // Padding
            }
        }
        
        Ok(())
    }

    /// Add metadata to encoder before encoding
    fn add_metadata_to_encoder(&self, encoder: &mut WavpackEncoder, metadata: &Metadata) -> R<()> {
        if encoder.context.is_null() {
            return Err(anyhow!("Encoder not initialized"));
        }

        // First, restore wrapper data if available, or generate it from source format
        if let Some(wrapper_hex) = metadata.get_field("WAVPACK_WRAPPER_DATA") {
            // We have existing WavPack wrapper data - restore it
            if let Some(wrapper_size_str) = metadata.get_field("WAVPACK_WRAPPER_SIZE") {
                if let Ok(wrapper_size) = wrapper_size_str.parse::<usize>() {
                    // Convert hex string back to bytes
                    let mut wrapper_data = Vec::with_capacity(wrapper_size);
                    for chunk in wrapper_hex.as_bytes().chunks(2) {
                        if chunk.len() == 2 {
                            let hex_str = std::str::from_utf8(chunk).unwrap_or("00");
                            if let Ok(byte_val) = u8::from_str_radix(hex_str, 16) {
                                wrapper_data.push(byte_val);
                            }
                        }
                    }
                    
                    if wrapper_data.len() == wrapper_size {
                        dprintln!("WavPack embed: Restoring {} bytes of wrapper data", wrapper_size);
                        
                        // Use WavPack API to restore wrapper data
                        let result = unsafe {
                            WavpackAddWrapper(
                                encoder.context,
                                wrapper_data.as_ptr() as *mut c_void,
                                wrapper_size as uint32_t,
                            )
                        };
                        
                        if result != 0 {
                            dprintln!("WavPack embed: Successfully restored wrapper data");
                        } else {
                            dprintln!("WavPack embed: Warning - failed to restore wrapper data");
                        }
                    } else {
                        dprintln!("WavPack embed: Warning - wrapper data size mismatch: {} vs {}", wrapper_data.len(), wrapper_size);
                    }
                }
            }
        } else {
            // No existing wrapper data - generate it from source format metadata
            dprintln!("WavPack embed: No wrapper data found, generating from source format metadata");
            
            if let Ok(generated_wrapper) = self.generate_wrapper_data_from_metadata(metadata) {
                if !generated_wrapper.is_empty() {
                    dprintln!("WavPack embed: Generated {} bytes of wrapper data", generated_wrapper.len());
                    
                    let result = unsafe {
                        WavpackAddWrapper(
                            encoder.context,
                            generated_wrapper.as_ptr() as *mut c_void,
                            generated_wrapper.len() as uint32_t,
                        )
                    };
                    
                    if result != 0 {
                        dprintln!("WavPack embed: Successfully added generated wrapper data");
                    } else {
                        dprintln!("WavPack embed: Warning - failed to add generated wrapper data");
                    }
                }
            }
        }

        // Restore MD5 checksum if available
        if let Some(md5_hex) = metadata.get_field("WAVPACK_MD5") {
            if md5_hex.len() == 32 { // 16 bytes * 2 chars per byte
                let mut md5_bytes = [0u8; 16];
                let mut hex_valid = true;
                
                for (i, chunk) in md5_hex.as_bytes().chunks(2).enumerate() {
                    if i >= 16 { break; }
                    if chunk.len() == 2 {
                        let hex_str = std::str::from_utf8(chunk).unwrap_or("00");
                        if let Ok(byte_val) = u8::from_str_radix(hex_str, 16) {
                            md5_bytes[i] = byte_val;
                        } else {
                            hex_valid = false;
                            break;
                        }
                    }
                }
                
                if hex_valid {
                    let result = unsafe {
                        WavpackStoreMD5Sum(encoder.context, md5_bytes.as_mut_ptr())
                    };
                    
                    if result != 0 {
                        dprintln!("WavPack embed: Successfully restored MD5 checksum");
                    } else {
                        dprintln!("WavPack embed: Warning - failed to restore MD5 checksum");
                    }
                }
            }
        }

        // Add text fields from the hashmap
        for (key, value) in metadata.get_all_fields().iter() {
            // Skip internal WavPack metadata fields
            if key.starts_with("WAVPACK_") {
                continue;
            }
            
            // Check if this is a TAG_ prefixed key (from embedded text tags)
            let wavpack_key = if let Some(unprefixed_key) = key.strip_prefix("TAG_") {
                // Remove TAG_ prefix and map the remaining key to WavPack format
                self.map_to_wavpack_key(unprefixed_key)
            } else if key.starts_with("INFO_") {
                // Handle WAV LIST INFO chunks from wrapper data
                continue; // These are handled by wrapper data restoration
            } else {
                // This is likely an iXML field, skip it here as it will be handled in iXML creation
                continue;
            };
            
            let c_key =
                CString::new(wavpack_key.as_str()).map_err(|_| anyhow!("Invalid metadata key"))?;
            let trimmed_value = value.trim();
            let c_value =
                CString::new(trimmed_value).map_err(|_| anyhow!("Invalid metadata value"))?;

            let result = unsafe {
                WavpackAppendTagItem(
                    encoder.context,
                    c_key.as_ptr(),
                    c_value.as_ptr(),
                    trimmed_value.len() as c_int,
                )
            };

            if result != 1 {
                dprintln!(
                    "Warning: Failed to add text tag '{}' - result: {}",
                    wavpack_key,
                    result
                );
            } else {
                dprintln!(
                    "Successfully added text tag '{}' with value '{}'",
                    wavpack_key,
                    trimmed_value
                );
            }
        }

        // Create and add iXML from all metadata fields
        if !metadata.get_all_fields().is_empty() {
            let ixml_content = self.create_ixml(metadata)?;
            let c_key = CString::new("iXML").map_err(|_| anyhow!("Invalid metadata key"))?;
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
                dprintln!("Warning: Failed to add iXML tag - result: {}", result);
            } else {
                dprintln!("Successfully added iXML tag");
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

            let c_item = CString::new(item_name).map_err(|_| anyhow!("Invalid item name"))?;

            let result = unsafe {
                WavpackAppendBinaryTagItem(
                    encoder.context,
                    c_item.as_ptr(),
                    image.data().as_ptr() as *const c_char,
                    image.data().len() as c_int,
                )
            };

            if result != 1 {
                dprintln!(
                    "Warning: Failed to add picture '{}' - result: {}",
                    image.description(),
                    result
                );
            } else {
                dprintln!("Successfully added picture '{}'", image.description());
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
                if key.starts_with("USER_")
                    || key.starts_with("BEXT_")
                    || key.starts_with("ASWG_")
                    || key.starts_with("STEINBERG_")
                {
                    key.to_string()
                } else {
                    key.to_string()
                }
            }
        }
    }

    /// Map standard metadata field names to WavPack tag names
    fn map_to_wavpack_key(&self, key: &str) -> String {
        // Preserve original case for Soundminer compatibility
        match key {
            "Year" => "Date".to_string(),
            "Track" => "TrackNumber".to_string(),
            "DiscNumber" => "DiscNumber".to_string(),
            "Publisher" => "Organization".to_string(),
            "EncodingSettings" => "Encoder".to_string(),
            // For most keys, preserve the original case as it appears in TAG_ prefix
            // This ensures Soundminer can read the metadata correctly
            _ => key.to_string(),
        }
    }
}
