// pub mod decode;
mod codecs;
use codecs::*;
mod prelude;
use crate::prelude::*;
mod resample;

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
        self.buffer = codec.decode(&mapped_file)?;
        self.metadata = codec.extract_metadata_from_file(input_file)?;
        self.codec = Some(codec);
        Ok(())
    }

    pub fn resample(&mut self, new_rate: u32) {
        self.buffer.resample(new_rate);
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
            Ok(_) => {
                println!("Successfully renamed temp file to: {}", output_file);
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
                    println!("Copy+delete successful");
                    Ok(())
                }
            }
        }
    }

    pub fn convert_dual_mono(&mut self) -> R<()> {
        // First, modify the audio buffer to remove duplicate channels
        self.buffer.strip_multi_mono()?;

        // Now update metadata to reflect the new channel count
        // Look for metadata chunks that might contain channel information
        // for chunk in &mut self.metadata {
        //     match chunk {
        //         MetadataChunk::IXml(xml) => {
        //             // Update channel references in XML
        //             *xml = xml
        //                 .replace("CHANNELS=2", "CHANNELS=1")
        //                 .replace("channels=2", "channels=1")
        //                 .replace("NumChannels=2", "NumChannels=1");
        //         }
        //         MetadataChunk::Bext(data) if data.len() >= 356 => {
        //             // Update channel count in BEXT chunk (at offset 354-355)
        //             data[354] = 1;
        //             data[355] = 0; // Little-endian representation of 1
        //         }
        //         // Add other format-specific metadata updates as needed
        //         _ => {}
        //     }
        // }

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
        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        self.decode(&mapped_file)
    }

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata>;

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>>;

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>>;

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()>;
}
