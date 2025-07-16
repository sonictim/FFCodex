// pub mod decode;
pub mod codecs;
use std::{collections::HashMap, hash::Hash, path::PathBuf};

use codecs::*;
mod prelude;
use crate::prelude::*;
mod chromaprint;
pub mod chromaprint_bindings;
pub mod resample;
pub mod wavpack_bindings;

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
        let codec = get_codec(path)?;
        if codec.file_extension() == "wv" {
            let mut codex = Codex::open(path)?;
            codex.convert_dual_mono()?;
            codex.export(temp_path.to_str().unwrap())?;
        } else {
            let mut buffer = codec.decode_file(path)?; // Load once
            let metadata = codec.extract_metadata_from_file(path)?;
            buffer.strip_multi_mono()?; // Process in-place
            codec.encode_file(&Some(buffer), temp_path.to_str().unwrap())?; // Write once
            codec.embed_metadata_to_file(temp_path.to_str().unwrap(), &metadata)?;
        } // All memory freed here
    }

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

    pub fn embed_metadata(self, file_path: &str) -> R<Self> {
        if self.metadata.is_none() {
            self.extract_metadata()?;
        }
        let Some(metadata) = &self.metadata else {
            return Err(anyhow::anyhow!("No metadata available to embed"));
        };
        let codec = self.codec.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "No codec available for encoding metadata to file: {}",
                self.path.display()
            )
        })?;
        codec.embed_metadata_to_file(file_path, metadata)?;

        Ok(self)
    }



    pub fn set_metadata_field(&mut self, key: &str, value: &str) -> R<()> {
        match &mut self.metadata {
            Some(metadata) => {
                metadata.set_field(key, value);
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
                
                // Embed metadata if available
                if let Some(metadata) = &self.metadata {
                    codec.embed_metadata_to_file(temp_path, metadata)?;
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
}

pub trait Codec: Send + Sync {
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
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };
        self.parse_metadata(&mapped_file)
    }

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata>;

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()>;
}
