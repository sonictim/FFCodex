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
        let start_time = std::time::Instant::now();

        println!("Opening file: {}", input_file);
        let codec_start = std::time::Instant::now();
        let codec = get_codec(input_file)?;
        println!(
            "Codec creation took: {:.2}ms",
            codec_start.elapsed().as_millis()
        );

        let file_start = std::time::Instant::now();
        let file = std::fs::File::open(input_file)?;
        let file_size = file.metadata()?.len();
        println!(
            "File open took: {:.2}ms, size: {:.1}MB",
            file_start.elapsed().as_millis(),
            file_size as f64 / 1_000_000.0
        );

        let mmap_start = std::time::Instant::now();
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };
        println!(
            "Memory mapping took: {:.2}ms",
            mmap_start.elapsed().as_millis()
        );

        let metadata_start = std::time::Instant::now();
        self.metadata = codec.extract_metadata_from_file(input_file)?;
        println!(
            "Metadata extraction took: {:.2}ms",
            metadata_start.elapsed().as_millis()
        );

        let decode_start = std::time::Instant::now();
        self.buffer = codec.decode(&mapped_file)?;
        let decode_duration = decode_start.elapsed();
        println!(
            "Audio decode took: {:.2}ms ({:.1}MB/s)",
            decode_duration.as_millis(),
            file_size as f64 / 1_000_000.0 / decode_duration.as_secs_f64()
        );

        self.codec = Some(codec);
        self.path = PathBuf::from(input_file);

        let total_duration = start_time.elapsed();
        println!("Total file open took: {:.2}ms", total_duration.as_millis());
        Ok(())
    }

    pub fn get_filename(&self) -> &str {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
    }

    pub fn resample(&mut self, new_rate: u32) {
        let start_time = std::time::Instant::now();
        self.buffer.resample(new_rate);
        println!(
            "Total resample operation took: {:.2}ms",
            start_time.elapsed().as_millis()
        );
    }

    pub fn change_bit_depth(&mut self, new_bit_depth: u16) {
        self.buffer.change_bit_depth(new_bit_depth);
    }

    pub fn export(&self, output_file: &str) -> R<()> {
        let start_time = std::time::Instant::now();

        let temp_file = std::env::temp_dir().join("temp_audio_file");
        let temp_path = temp_file.to_str().unwrap_or("");

        println!("Exporting to: {}", output_file);

        match get_codec(output_file) {
            Ok(codec) => {
                let encode_start = std::time::Instant::now();
                codec.encode_file(&self.buffer, temp_path)?;
                println!(
                    "Audio encode took: {:.2}ms",
                    encode_start.elapsed().as_millis()
                );

                let metadata_start = std::time::Instant::now();
                codec.embed_metadata_to_file(temp_path, &self.metadata)?;
                println!(
                    "Metadata embed took: {:.2}ms",
                    metadata_start.elapsed().as_millis()
                );
            }
            Err(error) => return Err(error),
        }

        let rename_start = std::time::Instant::now();
        match std::fs::rename(&temp_file, output_file) {
            Ok(_) => {
                println!(
                    "File rename took: {:.2}ms",
                    rename_start.elapsed().as_millis()
                );
                println!("Successfully renamed temp file to: {}", output_file);
                println!(
                    "Total export took: {:.2}ms",
                    start_time.elapsed().as_millis()
                );
                Ok(())
            }
            Err(e) => {
                // If rename fails, try to analyze and provide a helpful error
                let error_message = match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        format!("Permission denied when renaming to {}", output_file)
                    }
                    std::io::ErrorKind::NotFound => format!(
                        "Temporary file disappeared during rename: {}",
                        temp_file.display()
                    ),
                    std::io::ErrorKind::CrossesDevices => "Cannot rename across different volumes - this shouldn't happen with our approach".to_string(),
                    _ => format!("Error renaming temp file: {}", e),
                };

                println!("{}", error_message);

                // As a fallback, try to copy then delete
                println!("Attempting copy+delete as fallback...");
                if let Err(copy_err) = std::fs::copy(&temp_file, output_file) {
                    println!("Copy failed: {}", copy_err);
                    Err(e.into()) // Return the original error
                } else {
                    let _ = std::fs::remove_file(&temp_file); // Try to cleanup
                    println!(
                        "Copy+delete successful, took: {:.2}ms",
                        rename_start.elapsed().as_millis()
                    );
                    println!(
                        "Total export took: {:.2}ms",
                        start_time.elapsed().as_millis()
                    );
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
