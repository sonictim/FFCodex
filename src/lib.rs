// pub mod decode;
pub mod codecs;
use std::path::PathBuf;

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
    let mut c = Codex::open(path)?;
    c.convert_dual_mono()?;
    c.export(path)?;
    Ok(())
}

pub fn get_fingerprint(path: &str) -> R<String> {
    Codex::new(path)?.decode()?.get_chromaprint_fingerprint()
}

#[derive(Debug)]
pub enum Metadata {
    // Chunk-based formats (WAV, AIFF, WavPack) all use the same structure
    // since they share similar metadata chunk architectures
    Wav(Vec<MetadataChunk>), // Also used for AIFF and WavPack

    // FLAC has a unique metadata structure with Vorbis comments,
    // picture blocks, etc. that doesn't map well to chunks
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

    // pub fn new(input_file: &str) -> Self {
    //     let mut codex = Self::default();
    //     match codex.open(input_file) {
    //         Ok(_) => codex,
    //         Err(e) => {
    //             codex.error = Some(e);
    //             codex
    //         }
    //     }
    // }

    fn open(input_file: &str) -> R<Self> {
        Self::new(input_file)?.decode()?.extract_metadata()
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
                // For WavPack files, we need to encode with metadata included during encoding
                // to avoid the issue where encode_file overwrites metadata
                if output_file.to_lowercase().ends_with(".wv") {
                    // Extract metadata chunks for WavPack
                    if let Some(Metadata::Wav(chunks)) = &self.metadata {
                        dprintln!(
                            "🎯 Codex export: WavPack detected with {} metadata chunks",
                            chunks.len()
                        );

                        // Show the order of chunks being passed to the codec
                        for (i, chunk) in chunks.iter().enumerate() {
                            match chunk {
                                MetadataChunk::TextTag { key, .. } => {
                                    dprintln!(
                                        "🎯 Codex export: Passing chunk [{}] TextTag: {}",
                                        i,
                                        key
                                    );
                                }
                                MetadataChunk::Picture { description, .. } => {
                                    dprintln!(
                                        "🎯 Codex export: Passing chunk [{}] Picture: {}",
                                        i,
                                        description
                                    );
                                }
                                MetadataChunk::Unknown { id, .. } => {
                                    dprintln!(
                                        "🎯 Codex export: Passing chunk [{}] Unknown: {}",
                                        i,
                                        id
                                    );
                                }
                                _ => {
                                    dprintln!(
                                        "🎯 Codex export: Passing chunk [{}] Other: {}",
                                        i,
                                        chunk.id()
                                    );
                                }
                            }
                        }

                        // Create a dummy input buffer to use with embed_metadata_chunks
                        let encoded_audio = codec.encode(&self.buffer)?;
                        let encoded_with_metadata =
                            codec.embed_metadata_chunks(&encoded_audio, chunks)?;
                        std::fs::write(temp_path, encoded_with_metadata)?;
                    } else {
                        dprintln!("🎯 Codex export: WavPack detected but no WAV metadata to embed");
                        // No metadata to embed, just encode normally
                        codec.encode_file(&self.buffer, temp_path)?;
                    }
                } else {
                    // For other formats, use the original approach
                    codec.encode_file(&self.buffer, temp_path)?;
                    codec.embed_metadata_to_file(temp_path, &self.metadata)?;
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

    pub fn parse_metadata(&self) -> R<()> {
        match &self.metadata {
            Some(Metadata::Flac(tag)) => {
                dprintln!("Parsing FLAC metadata");
                if let Some(comments) = tag.vorbis_comments() {
                    dprintln!("FLAC Vorbis Comments found: {:?}", comments.comments);
                } else {
                    dprintln!("No Vorbis Comments found in FLAC metadata");
                }

                return Ok(());
            }
            Some(Metadata::Wav(chunks)) => {
                for chunk in chunks {
                    // if chunk.id() == "SMED" {
                    //     dprintln!("{:?}", chunk);
                    // }
                    dprintln!("Parsing metadata chunk: {:?}", chunk.id());
                    let map = chunk.parse()?;
                    dprintln!("Parsed metadata chunk: {:?}", map);
                }
            }
            None => {
                dprintln!("No metadata available to parse");
                return Ok(());
            }
        }
        Ok(())
    }
}

pub trait Codec: Send + Sync {
    fn validate_file_format(&self, data: &[u8]) -> R<()>;
    fn file_extension(&self) -> &'static str;

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

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata>;

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>>;

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>>;

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Option<Metadata>) -> R<()>;
}
