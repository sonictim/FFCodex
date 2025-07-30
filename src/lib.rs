// pub mod decode;
pub mod codecs;
pub mod ixml;
use std::path::PathBuf;

use codecs::*;
mod prelude;
use crate::prelude::*;
pub mod bindings;
mod chromaprint;
pub mod resample;
pub mod soundminer;

// Standard bit depths
const BIT_DEPTH_8: u16 = 8;
const BIT_DEPTH_16: u16 = 16;
const BIT_DEPTH_24: u16 = 24;
const BIT_DEPTH_32: u16 = 32;

// Sample normalization constants
const U8_OFFSET: f32 = 128.0;
const U8_SCALE: f32 = 127.0;
const I16_MAX_F: f32 = 32767.0;
const I16_DIVISOR: f32 = 32768.0;
const I16_DIVISOR_RECIP: f32 = 1.0 / 32768.0; // Pre-calculated reciprocal
const I24_MAX_F: f32 = 8388607.0;
const I24_DIVISOR: f32 = 8388608.0;
const I24_DIVISOR_RECIP: f32 = 1.0 / 8388608.0; // Pre-calculated reciprocal
const I32_MAX_F: f32 = 2147483647.0;
const I32_DIVISOR: f32 = 2147483648.0;
const I32_DIVISOR_RECIP: f32 = 1.0 / 2147483648.0; // Pre-calculated reciprocal

//Bit Operations
const I24_SIGN_BIT: i32 = 0x800000;
const I24_SIGN_EXTENSION_MASK: i32 = !0xFFFFFF;
const BYTE_MASK: i32 = 0xFF; // Mask for extracting a single byte

pub fn debug_println(args: std::fmt::Arguments) {
    if cfg!(debug_assertions) {
        println!("{}", args);
    }
}

// Helper macro to use it like println!
#[macro_export]
macro_rules! dprintln {
    ($($arg:tt)*) => {
        $crate::debug_println(format_args!($($arg)*))
    };
}
pub fn clean_multi_mono(path: &str) -> R<()> {
    let temp_path = std::env::temp_dir().join(format!(
        "ffcodex_{}",
        PathBuf::from(path).file_name().unwrap().to_string_lossy()
    ));

    // Process in chunks to minimize memory usage
    {
        let mut codex = Codex::open(path)?;
        codex.convert_dual_mono()?;
        codex.export(temp_path.to_str().unwrap())?;
    } // All memory freed here

    // Replace original - use same robust logic as export()
    match std::fs::rename(&temp_path, path) {
        Ok(_) => Ok(()),
        Err(e) => {
            // As a fallback, try to copy then delete (Windows compatibility)
            if let Err(_copy_err) = std::fs::copy(&temp_path, path) {
                let _ = std::fs::remove_file(&temp_path); // Cleanup on failure
                Err(e.into()) // Return the original error
            } else {
                let _ = std::fs::remove_file(&temp_path); // Cleanup temp file
                Ok(())
            }
        }
    }
}

pub fn get_fingerprint(path: &str) -> R<String> {
    Codex::new(path)?.decode()?.get_chromaprint_fingerprint()
}

pub fn strip_soundminer_metadata(file_path: &str) -> R<()> {
    let path = PathBuf::from(file_path);
    if !path.exists() {
        return Err(anyhow::anyhow!("File does not exist: {}", file_path));
    }

    // Read the original file
    let original_data = std::fs::read(file_path)?;

    // Determine the file format and strip SMED chunks accordingly
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid file extension"))?;

    let cleaned_data = match extension.to_lowercase().as_str() {
        "flac" => strip_smed_from_flac(&original_data)?,
        "aif" | "aiff" => strip_smed_from_aiff(&original_data)?,
        "wav" => strip_smed_from_wav(&original_data)?,
        "wv" => strip_smed_from_wavpack(&original_data)?,
        _ => return Err(anyhow::anyhow!("Unsupported file format: {}", extension)),
    };

    // Create backup
    let backup_path = format!("{}.backup", file_path);
    std::fs::copy(file_path, backup_path)?;

    // Write cleaned data back to original file
    std::fs::write(file_path, cleaned_data)?;

    println!("Soundminer metadata stripped from: {}", file_path);
    println!("Backup created at: {}.backup", file_path);

    Ok(())
}

pub fn get_basic_metadata(path: &str) -> R<FileInfo> {
    let codex = Codex::new(path)?.extract_metadata()?;
    codex.get_file_info()
}

#[derive(Debug)]
pub struct FileInfo {
    pub path: String,
    pub size: usize,
    pub sample_rate: u16,
    pub channels: u16,
    pub bit_depth: u16,
    pub duration: String,
    pub description: String,
}

#[derive(Default)]
pub struct Codex {
    pub path: PathBuf,
    pub buffer: Option<AudioBuffer>,
    pub metadata: Option<Metadata>,
    pub codec: Option<Box<dyn Codec>>,
}

impl Codex {
    pub fn new(input_file: &str) -> R<Self> {
        let path = PathBuf::from(input_file);
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Input file does not exist: {}",
                path.display()
            ));
        }

        // let mut codex = Self::default();
        Ok(Self {
            path,
            codec: get_codec(input_file).ok(),
            metadata: None,
            buffer: None,
        })
    }

    fn open(input_file: &str) -> R<Self> {
        Self::new(input_file)?.decode()?.extract_metadata()
    }

    pub fn decode(mut self) -> R<Self> {
        let codec = self.codec.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "No codec available for decoding audio file: {}",
                self.path.display()
            )
        })?;
        let file = std::fs::File::open(&self.path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };
        self.buffer = Some(codec.decode(&mapped_file)?);
        Ok(self)
    }

    pub fn extract_metadata(mut self) -> R<Self> {
        let codec = self.codec.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "No codec available for extracting metadata from file: {}",
                self.path.display()
            )
        })?;
        self.metadata = Some(codec.extract_metadata_from_file(self.path.to_str().unwrap())?);
        Ok(self)
    }

    pub fn embed_metadata(&self) -> R<()> {
        let metadata = match &self.metadata {
            Some(metadata) => metadata,
            None => return Err(anyhow::anyhow!("No metadata available to embed")),
        };

        let Some(codec) = &self.codec else {
            return Err(anyhow::anyhow!(
                "No codec available for embedding metadata in file: {}",
                self.path.display()
            ));
        };

        codec.embed_metadata_to_file(self.path.to_str().unwrap(), metadata)
    }
    pub fn embed_metadata_to_different_file(&self, file_path: &str) -> R<()> {
        let metadata = match &self.metadata {
            Some(metadata) => metadata,
            None => return Err(anyhow::anyhow!("No metadata available to embed")),
        };

        // Get codec based on OUTPUT file extension, not input file
        let output_codec = get_codec(file_path)?;

        // If we have audio buffer, create a new file with audio + metadata
        if let Some(buffer) = &self.buffer {
            // First encode the audio to the target format
            let encoded_data = output_codec.encode(&self.buffer)?;

            // Write the encoded data to file
            std::fs::write(file_path, encoded_data)?;

            // Update metadata with the audio format information from the buffer
            let updated_metadata = self.update_metadata_from_buffer(metadata, buffer);

            // Then embed metadata to the newly created file
            output_codec.embed_metadata_to_file(file_path, &updated_metadata)?;
        } else {
            // If no audio buffer, just embed metadata (this might fail for some formats)
            output_codec.embed_metadata_to_file(file_path, metadata)?;
        }

        Ok(())
    }

    pub fn set_metadata_field(&mut self, key: &str, value: &str) -> R<()> {
        match &mut self.metadata {
            Some(metadata) => {
                metadata.set_field(key, value)?;
                Ok(())
            }
            None => Err(anyhow::anyhow!(
                "No metadata available to set field: {}",
                key
            )),
        }
    }

    pub fn get_metadata_field(&self, key: &str) -> Option<String> {
        match &self.metadata {
            Some(metadata) => metadata.get_field(key),
            None => None,
        }
    }

    pub fn get_filename(&self) -> &str {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
    }

    fn resample(&mut self, new_rate: u32) -> R<()> {
        if let Some(buffer) = &mut self.buffer {
            buffer.resample(new_rate);
            Ok(())
        } else {
            Err(anyhow::anyhow!("No audio buffer available for resampling"))
        }
    }

    fn change_bit_depth(&mut self, new_bit_depth: u16) -> R<()> {
        if let Some(buffer) = &mut self.buffer {
            buffer.change_bit_depth(new_bit_depth);
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "No audio buffer available for changing bit depth"
            ))
        }
    }

    pub fn export(&self, output_file: &str) -> R<()> {
        let temp_file = std::env::temp_dir().join("temp_audio_file");
        let temp_path = temp_file.to_str().unwrap_or("");

        match get_codec(output_file) {
            Ok(codec) => {
                // Encode the audio first
                codec.encode_file(&self.buffer, temp_path)?;

                // Embed metadata if available, updating it with current buffer info
                if let Some(metadata) = &self.metadata {
                    if let Some(buffer) = &self.buffer {
                        let updated_metadata = self.update_metadata_from_buffer(metadata, buffer);
                        codec.embed_metadata_to_file(temp_path, &updated_metadata)?;
                    } else {
                        codec.embed_metadata_to_file(temp_path, metadata)?;
                    }
                }
            }
            Err(error) => return Err(error),
        }

        match std::fs::rename(&temp_file, output_file) {
            Ok(_) => Ok(()),
            Err(e) => {
                // As a fallback, try to copy then delete
                if let Err(_copy_err) = std::fs::copy(&temp_file, output_file) {
                    Err(e.into()) // Return the original error
                } else {
                    let _ = std::fs::remove_file(&temp_file); // Try to cleanup
                    Ok(())
                }
            }
        }
    }

    pub fn convert_dual_mono(&mut self) -> R<()> {
        let Some(buffer) = &mut self.buffer else {
            return Err(anyhow::anyhow!(
                "No audio buffer available for dual mono conversion"
            ));
        };
        buffer.strip_multi_mono()?;

        // Update metadata to reflect the new channel count
        if let Some(metadata) = &mut self.metadata {
            metadata.channels = buffer.channels;
        }

        Ok(())
    }

    // Add helper methods to expose channel information
    pub fn channels(&self) -> R<u16> {
        let Some(buffer) = &self.buffer else {
            return Err(anyhow::anyhow!("No audio buffer available"));
        };
        Ok(buffer.channels)
    }

    pub fn data_channels(&self) -> R<usize> {
        let Some(buffer) = &self.buffer else {
            return Err(anyhow::anyhow!("No audio buffer available"));
        };
        Ok(buffer.data.len())
    }

    fn get_file_info(&self) -> R<FileInfo> {
        let codec = self.codec.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "No codec available for decoding audio file: {}",
                self.path.display()
            )
        })?;
        codec.get_file_info(self.path.to_str().unwrap())
    }

    fn update_metadata_from_buffer(&self, metadata: &Metadata, buffer: &AudioBuffer) -> Metadata {
        let mut updated_metadata = metadata.clone();
        updated_metadata.sample_rate = buffer.sample_rate;
        updated_metadata.channels = buffer.channels;
        updated_metadata.bit_depth = match buffer.format {
            SampleFormat::U8 => 8,
            SampleFormat::I16 => 16,
            SampleFormat::I24 => 24,
            SampleFormat::I32 => 32,
            SampleFormat::F32 => 32,
        };
        updated_metadata.format_tag = match buffer.format {
            SampleFormat::F32 => 3, // IEEE float
            _ => 1,                 // PCM
        };
        updated_metadata
    }
}

pub trait Codec: Send + Sync {
    fn as_str(&self) -> &'static str;
    fn validate_file_format(&self, data: &[u8]) -> R<()>;
    fn file_extension(&self) -> &'static str;
    fn get_file_info(&self, file_path: &str) -> R<FileInfo>;
    fn encode(&self, buffer: &Option<AudioBuffer>) -> R<Vec<u8>>;
    fn encode_file(&self, buffer: &Option<AudioBuffer>, file_path: &str) -> R<()> {
        let encoded_data = self.encode(buffer)?;
        std::fs::write(file_path, encoded_data)?;
        Ok(())
    }
    fn decode(&self, input: &[u8]) -> R<AudioBuffer>;
    fn decode_file(&self, file_path: &str) -> R<AudioBuffer> {
        use memmap2::Mmap;
        use std::fs::File;

        let mut file = File::open(file_path)?;
        let file_size = file.metadata()?.len();

        // Only use mmap for large files
        if file_size > 100 * 1024 * 1024 {
            // 100MB threshold
            let mmap = unsafe { Mmap::map(&file)? };
            self.decode(&mmap)
        } else {
            let mut data = vec![0; file_size as usize];
            file.read_exact(&mut data)?;
            self.decode(&data)
        }
    }

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata> {
        let file = std::fs::File::open(file_path)?;
        let file_size = file.metadata()?.len();

        // Use memory mapping only for files larger than 100MB
        if file_size > 100 * 1024 * 1024 {
            let mapped_file = unsafe { MmapOptions::new().map(&file)? };
            self.parse_metadata(&mapped_file)
        } else {
            use std::io::Read;
            let mut data = Vec::with_capacity(file_size as usize);
            let mut file = file;
            file.read_to_end(&mut data)?;
            self.parse_metadata(&data)
        }
    }

    fn create_ixml(&self, metadata: &Metadata) -> R<String> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push('<');
        xml.push_str(self.as_str());
        xml.push_str("XML>\n");
        xml.push_str(&ixml::create_ixml_from_metadata(metadata)?);
        xml.push_str("</");
        xml.push_str(self.as_str());
        xml.push_str("XML>\n");
        Ok(xml)
    }
    // fn create_ixml(&self, metadata: &Metadata) -> R<String> {
    //     let mut xml = String::new();
    //     xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    //     xml.push('<');
    //     xml.push_str(self.as_str());
    //     xml.push_str("XML>\n");
    //     xml.push_str(&ixml::create_ixml_from_metadata(metadata)?);
    //     xml.push_str("</");
    //     xml.push_str(self.as_str());
    //     xml.push_str("XML>\n");
    //     Ok(xml)
    // }

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata>;

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()>;
}

// Helper functions for stripping Soundminer metadata from different formats

fn strip_smed_from_flac(data: &[u8]) -> R<Vec<u8>> {
    if data.len() < 4 || &data[0..4] != b"fLaC" {
        return Err(anyhow::anyhow!("Not a valid FLAC file"));
    }

    let mut output = Vec::new();
    let mut cursor = Cursor::new(data);

    // Copy fLaC header
    output.extend_from_slice(b"fLaC");
    cursor.set_position(4);

    // Process metadata blocks
    while cursor.position() < data.len() as u64 {
        let pos = cursor.position() as usize;
        if pos + 4 > data.len() {
            break;
        }

        let block_header = data[pos];
        let is_last = (block_header & 0x80) != 0;
        let block_type = block_header & 0x7F;

        let block_size =
            ((data[pos + 1] as u32) << 16) | ((data[pos + 2] as u32) << 8) | (data[pos + 3] as u32);

        if pos + 4 + block_size as usize > data.len() {
            break;
        }

        let block_data = &data[pos + 4..pos + 4 + block_size as usize];

        // Check if this is a Soundminer APPLICATION block
        let is_smed_block = if block_type == 2 && block_size >= 4 {
            // APPLICATION block
            &block_data[0..4] == b"SMED"
        } else {
            false
        };

        if !is_smed_block {
            // Copy non-SMED blocks
            output.push(if is_last && block_header != data[pos] {
                block_header | 0x80 // Ensure last block flag is set if this becomes the last block
            } else {
                block_header
            });
            output.extend_from_slice(&data[pos + 1..pos + 4 + block_size as usize]);
        } else {
            println!("Removed SMED APPLICATION block ({} bytes)", block_size);
        }

        cursor.set_position(pos as u64 + 4 + block_size as u64);

        if is_last {
            break;
        }
    }

    // Copy remaining audio frames
    let remaining_pos = cursor.position() as usize;
    if remaining_pos < data.len() {
        output.extend_from_slice(&data[remaining_pos..]);
    }

    Ok(output)
}

fn strip_smed_from_aiff(data: &[u8]) -> R<Vec<u8>> {
    if data.len() < 12 || &data[0..4] != b"FORM" || &data[8..12] != b"AIFF" {
        return Err(anyhow::anyhow!("Not a valid AIFF file"));
    }

    let mut output = Vec::new();
    let mut cursor = Cursor::new(data);

    // Copy FORM/AIFF header
    output.extend_from_slice(&data[0..12]);
    cursor.set_position(12);

    let mut removed_bytes = 0u32;

    // Process chunks
    while cursor.position() + 8 <= data.len() as u64 {
        let pos = cursor.position() as usize;
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = ((data[pos + 4] as u32) << 24)
            | ((data[pos + 5] as u32) << 16)
            | ((data[pos + 6] as u32) << 8)
            | (data[pos + 7] as u32);

        let total_chunk_size = 8 + chunk_size as usize + (chunk_size as usize % 2); // Include padding

        if pos + total_chunk_size > data.len() {
            break;
        }

        // Check if this is a Soundminer chunk
        let is_smed_chunk = chunk_id == b"SMED"
            || (chunk_id == b"APPL" && chunk_size >= 4 && &data[pos + 8..pos + 12] == b"SMED");

        if !is_smed_chunk {
            // Copy non-SMED chunks
            output.extend_from_slice(&data[pos..pos + total_chunk_size]);
        } else {
            println!("Removed SMED chunk ({} bytes)", chunk_size);
            removed_bytes += 8 + chunk_size + (chunk_size % 2); // Include header and padding
        }

        cursor.set_position(pos as u64 + total_chunk_size as u64);
    }

    // Update FORM size in header
    if removed_bytes > 0 {
        let new_form_size = (output.len() as u32) - 8;
        output[4..8].copy_from_slice(&new_form_size.to_be_bytes());
    }

    Ok(output)
}

fn strip_smed_from_wav(data: &[u8]) -> R<Vec<u8>> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(anyhow::anyhow!("Not a valid WAV file"));
    }

    let mut output = Vec::new();
    let mut cursor = Cursor::new(data);

    // Copy RIFF/WAVE header
    output.extend_from_slice(&data[0..12]);
    cursor.set_position(12);

    let mut removed_bytes = 0u32;

    // Process chunks
    while cursor.position() + 8 <= data.len() as u64 {
        let pos = cursor.position() as usize;
        let chunk_id = &data[pos..pos + 4];
        let chunk_size =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);

        let total_chunk_size = 8 + chunk_size as usize + (chunk_size as usize % 2); // Include padding

        if pos + total_chunk_size > data.len() {
            break;
        }

        // Check if this is a Soundminer chunk (typically in LIST INFO or custom chunks)
        let is_smed_chunk = chunk_id == b"SMED"
            || (chunk_id == b"LIST" && chunk_size >= 8 && &data[pos + 8..pos + 12] == b"SMED");

        if !is_smed_chunk {
            // Copy non-SMED chunks
            output.extend_from_slice(&data[pos..pos + total_chunk_size]);
        } else {
            println!("Removed SMED chunk ({} bytes)", chunk_size);
            removed_bytes += 8 + chunk_size + (chunk_size % 2); // Include header and padding
        }

        cursor.set_position(pos as u64 + total_chunk_size as u64);
    }

    // Update RIFF size in header
    if removed_bytes > 0 {
        let new_riff_size = (output.len() as u32) - 8;
        output[4..8].copy_from_slice(&new_riff_size.to_le_bytes());
    }

    Ok(output)
}

fn strip_smed_from_wavpack(data: &[u8]) -> R<Vec<u8>> {
    if data.len() < 4 || &data[0..4] != b"wvpk" {
        return Err(anyhow::anyhow!("Not a valid WavPack file"));
    }

    // For WavPack, SMED metadata is typically stored as text tags
    // We'll use a simpler approach - copy the file and remove SMED tags via WavPack API
    // This is more complex to implement directly, so for now return the original data
    // and suggest using WavPack's tag removal functionality

    println!("WavPack SMED removal not yet implemented - use WavPack tools to remove SMED tags");
    Ok(data.to_vec())
}
