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
            codec.embed_metadata_to_file(temp_path.to_str().unwrap(), &Some(metadata))?;
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

#[derive(Debug)]
pub enum Metadata {
    // Chunk-based formats (WAV, AIFF, WavPack) all use the same structure
    // since they share similar metadata chunk architectures
    Wav(Vec<MetadataChunk>), // Also used for AIFF and WavPack

    // FLAC has a unique metadata structure with Vorbis comments,
    // picture blocks, etc. that doesn't map well to chunks
    Flac(metaflac::Tag, Vec<MetadataChunk>), // FLAC metadata with Vorbis comments
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
        dprintln!("Parsing metadata for file: {}", self.path.display());
        match &self.metadata {
            Some(Metadata::Flac(tag, chunks)) => {
                dprintln!("Parsing FLAC metadata");
                if let Some(comments) = tag.vorbis_comments() {
                    dprintln!("FLAC Vorbis Comments found: {:?}", comments.comments);
                } else {
                    dprintln!("No Vorbis Comments found in FLAC metadata");
                }

                // Only parse the first relevant metadata chunk (iXML or Vorbis) for FLAC
                for chunk in chunks {
                    if matches!(chunk, MetadataChunk::IXml(_)) {
                        dprintln!("Parsing first iXML metadata chunk: {:?}", chunk.id());
                        let map = chunk.parse()?;
                        dprintln!("Parsed iXML metadata chunk: {:?}", map);
                        break; // Stop after parsing the first iXML chunk
                    }
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

    pub fn set_metadata_field(&mut self, key: &str, value: &str) -> R<()> {
        let metadata = self
            .metadata
            .get_or_insert_with(|| Metadata::Wav(Vec::new()));

        match metadata {
            Metadata::Wav(chunks) => {
                dprintln!(
                    "🔍 set_metadata_field: Looking for key '{}' in {} chunks",
                    key,
                    chunks.len()
                );

                // Debug: Show what chunks we have
                for (i, chunk) in chunks.iter().enumerate() {
                    dprintln!(
                        "🔍 Chunk [{}]: {} (has field: {})",
                        i,
                        chunk.id(),
                        chunk.get_field(key).is_some()
                    );
                }

                // Try to find existing chunk with this field
                for chunk in chunks.iter_mut() {
                    if chunk.get_field(key).is_some() {
                        dprintln!(
                            "✅ Found existing chunk {} with field '{}', updating...",
                            chunk.id(),
                            key
                        );
                        return chunk.set_field(key, value);
                    }
                }

                // Look for iXML chunk to add the field to
                for chunk in chunks.iter_mut() {
                    if chunk.id() == "iXML" {
                        dprintln!("📝 Adding field '{}' to existing iXML chunk", key);
                        return chunk.set_field(key, value);
                    }
                }

                dprintln!("➕ Creating new TextTag chunk for key '{}'", key);
                // Create new TextTag chunk if field not found
                chunks.push(MetadataChunk::TextTag {
                    key: key.to_string(),
                    value: value.to_string(),
                });
                Ok(())
            }
            Metadata::Flac(tag, chunks) => {
                // For FLAC, work with the first relevant metadata chunk (iXML or Vorbis)
                // Also update the metaflac Vorbis comment
                tag.set_vorbis(key, vec![value.to_string()]);

                dprintln!(
                    "🔍 FLAC set_metadata_field: Looking for key '{}' in first relevant chunk only",
                    key
                );

                // Find and update only the first relevant metadata chunk (iXML)
                for chunk in chunks.iter_mut() {
                    if matches!(chunk, MetadataChunk::IXml(_)) {
                        dprintln!("📝 Found first iXML chunk, updating field '{}'", key);

                        // Debug: Print the chunk content before and after update
                        if let MetadataChunk::IXml(content) = chunk {
                            dprintln!(
                                "🔍 iXML content before update (first 200 chars): {}",
                                &content[..content.len().min(200)]
                            );
                        }

                        let result = chunk.set_field(key, value);

                        if let MetadataChunk::IXml(content) = chunk {
                            dprintln!(
                                "🔍 iXML content after update (first 200 chars): {}",
                                &content[..content.len().min(200)]
                            );
                        }

                        return result;
                    }
                }

                // If no iXML chunk exists, create a minimal one with just this field
                dprintln!(
                    "➕ No iXML chunk found, creating new one with field '{}'",
                    key
                );
                let xml_content = if key == "USER_DESIGNER" {
                    format!(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<BWFXML>
  <IXML_VERSION>1.0</IXML_VERSION>
  <USER>
    <USER_DESIGNER>{}</USER_DESIGNER>
  </USER>
</BWFXML>"#,
                        value
                    )
                } else {
                    // For other fields, use a generic structure
                    format!(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<BWFXML>
  <IXML_VERSION>1.0</IXML_VERSION>
  <USER>
    <{}>{}</{}>
  </USER>
</BWFXML>"#,
                        key, value, key
                    )
                };
                chunks.push(MetadataChunk::IXml(xml_content));
                Ok(())
            }
        }
    }

    pub fn get_metadata_field(&self, key: &str) -> Option<String> {
        match &self.metadata {
            Some(Metadata::Wav(chunks)) => chunks.iter().find_map(|chunk| chunk.get_field(key)),
            Some(Metadata::Flac(tag, chunks)) => {
                // First check Vorbis comments in the metaflac tag
                let mut result = tag
                    .vorbis_comments()
                    .and_then(|comments| comments.get(key))
                    .and_then(|values| values.first())
                    .map(|s| s.to_string());

                // If not found in Vorbis comments, check only the first relevant chunk (iXML)
                if result.is_none() {
                    for chunk in chunks.iter() {
                        if matches!(chunk, MetadataChunk::IXml(_)) {
                            result = chunk.get_field(key);
                            break; // Only check the first iXML chunk
                        }
                    }
                }

                result
            }
            None => None,
        }
    }

    // pub fn remove_metadata_field(&mut self, key: &str) -> R<bool> {
    //     match &mut self.metadata {
    //         Some(Metadata::Wav(chunks)) => {
    //             let initial_len = chunks.len();
    //             chunks.retain(|chunk| {
    //                 !matches!(
    //                     chunk,
    //                     MetadataChunk::TextTag { key: k, .. } if k == key
    //                 )
    //             });
    //             Ok(chunks.len() != initial_len)
    //         }
    //         Some(Metadata::Flac(tag)) => {
    //             if let Some(mut comments) = tag.vorbis_comments().cloned() {
    //                 let had_field = comments.get(key).is_some();
    //                 comments.remove(key);
    //                 tag.set_vorbis_comments(comments);
    //                 Ok(had_field)
    //             } else {
    //                 Ok(false)
    //             }
    //         }
    //         None => Ok(false),
    //     }
    //
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

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata>;

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>>;

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>>;

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Option<Metadata>) -> R<()>;
}
