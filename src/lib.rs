// pub mod decode;
mod codecs;
use std::path::PathBuf;

use codecs::*;
mod prelude;
use crate::prelude::*;
mod chromaprint;
pub mod chromaprint_bindings;
pub mod resample;

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
const I24_MAX_F: f32 = 8388607.0;
const I24_DIVISOR: f32 = 8388608.0;
const I32_MAX_F: f32 = 2147483647.0;
const I32_DIVISOR: f32 = 2147483648.0;

//Bit Operations
const I24_SIGN_BIT: i32 = 0x800000;
const I24_SIGN_EXTENSION_MASK: i32 = !0xFFFFFF;
const BYTE_MASK: i32 = 0xFF; // Mask for extracting a single byte

pub fn clean_multi_mono(path: &str) -> R<()> {
    let mut c = Codex::new(path);
    c.convert_dual_mono()?;
    c.export(path)?;
    Ok(())
}

pub fn get_fingerprint(path: &str) -> R<String> {
    let mut codex = Codex::default();
    match codex.open(path) {
        Ok(_) => codex.get_chromaprint_fingerprint(),
        Err(e) => {
            println!("Failed to Open");
            println!("Error: {}", e);
            Ok("FAILED".to_string())
        }
    }
}

#[derive(Debug)]
pub enum Metadata {
    Wav(Vec<MetadataChunk>),
    Flac(metaflac::Tag),
}

impl Default for Metadata {
    fn default() -> Self {
        Metadata::Wav(Vec::new())
    }
}

#[derive(Default)]
pub struct Codex {
    pub path: PathBuf,
    pub buffer: AudioBuffer,
    pub metadata: Metadata,
    pub codec: Option<Box<dyn Codec>>,
    error: Option<anyhow::Error>,
}

impl Codex {
    pub fn new(input_file: &str) -> Self {
        let mut codex = Self::default();
        match codex.open(input_file) {
            Ok(_) => codex,
            Err(e) => {
                codex.error = Some(e);
                codex
            }
        }
    }

    fn open(&mut self, input_file: &str) -> R<()> {
        let codec = get_codec(input_file)?;
        let file = std::fs::File::open(input_file)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };
        self.metadata = codec.extract_metadata_from_file(input_file)?;
        self.buffer = codec.decode(&mapped_file)?;
        self.codec = Some(codec);
        self.path = PathBuf::from(input_file);
        Ok(())
    }

    pub fn get_filename(&self) -> &str {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
    }

    pub fn resample(&mut self, new_rate: u32) {
        self.buffer.resample(new_rate);
    }

    pub fn change_bit_depth(&mut self, new_bit_depth: u16) {
        self.buffer.change_bit_depth(new_bit_depth);
    }

    pub fn export(&self, output_file: &str) -> R<()> {
        let temp_file = std::env::temp_dir().join("temp_audio_file");
        let temp_path = temp_file.to_str().unwrap_or("");

        match get_codec(output_file) {
            Ok(codec) => {
                codec.encode_file(&self.buffer, temp_path)?;
                codec.embed_metadata_to_file(temp_path, &self.metadata)?;
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
        self.buffer.strip_multi_mono()?;

        Ok(())
    }

    // Add helper methods to expose channel information
    pub fn channels(&self) -> u16 {
        self.buffer.channels
    }

    pub fn data_channels(&self) -> usize {
        self.buffer.data.len()
    }
    pub fn copy_metadata(&self, path: &str) -> R<()> {
        let codec = get_codec(path)?;
        if codec.file_extension() == self.codec.as_ref().unwrap().file_extension() {
            codec.embed_metadata_to_file(path, &self.metadata)?;
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Cannot copy metadata between different file formats"
            ))
        }
    }
}

pub trait Codec: Send + Sync {
    fn validate_file_format(&self, data: &[u8]) -> R<()>;
    fn file_extension(&self) -> &'static str;

    fn encode(&self, buffer: &AudioBuffer) -> R<Vec<u8>>;

    fn encode_file(&self, buffer: &AudioBuffer, file_path: &str) -> R<()> {
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

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata>;

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>>;

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>>;

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()>;
}
