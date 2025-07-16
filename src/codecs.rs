
use crate::prelude::*;
mod aif;
mod flac;
// mod mp3;
mod wav;
mod wavpack;
pub use aif::AifCodec;
pub use flac::FlacCodec;
// pub use mp3::Mp3Codec;
pub use wav::WavCodec;
pub use wavpack::WvCodec;

pub fn get_codec(file_path: &str) -> R<Box<dyn Codec>> {
    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid file extension"))?;

    match extension.to_lowercase().as_str() {
        "wav" => Ok(Box::new(WavCodec)),
        "flac" => Ok(Box::new(FlacCodec)),
        "aif" => Ok(Box::new(AifCodec)),
        "aiff" => Ok(Box::new(AifCodec)),
        "wv" => Ok(Box::new(WvCodec)),
        // "mp3" => Ok(Box::new(Mp3Codec)),
        _ => Err(anyhow::anyhow!(
            "No codec found for extension: {}",
            extension
        )),
    }
}

#[derive(Debug, Default, Clone)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
    pub data: Vec<Vec<f32>>, // deinterleaved float audio
}

impl AudioBuffer {
    pub fn resample(&mut self, new_rate: u32) {
        if self.sample_rate != new_rate {
            // Resample each channel individually using optimized functions
            for i in 0..self.data.len() {
                // Try fast common ratios first, fall back to optimized general algorithm
                self.data[i] = resample::resample_fast_common_ratios(
                    &self.data[i],
                    self.sample_rate,
                    new_rate,
                )
                .unwrap_or_else(|| {
                    resample::resample_optimized(&self.data[i], self.sample_rate, new_rate)
                });
            }

            self.sample_rate = new_rate;
        }
    }
    pub fn resample_channel(&mut self, i: usize, new_rate: u32) -> Vec<f32> {
        if self.sample_rate != new_rate {
            resample::resample_windowed_sinc(&self.data[i], self.sample_rate, new_rate)
        } else {
            self.data[i].clone()
        }
    }
    pub fn change_bit_depth(&mut self, new_bit_depth: u16) {
        if self.format.bits_per_sample() != new_bit_depth {
            for i in 0..self.data.len() {
                self.data[i] = resample::change_bit_depth(
                    &self.data[i],
                    self.sample_rate,
                    new_bit_depth as u32,
                    true,
                );
            }
            self.format = match new_bit_depth {
                8 => SampleFormat::U8,
                16 => SampleFormat::I16,
                24 => SampleFormat::I24,
                32 => SampleFormat::F32,
                _ => SampleFormat::F32,
            };
        }
    }

    pub fn strip_multi_mono(&mut self) -> R<()> {
        if self.data.is_empty() || self.channels < 2 {
            return Err(anyhow::anyhow!(
                "Cannot strip multi-mono: no data or less than 2 channels"
            ));
        }

        let first_channel = std::mem::take(&mut self.data[0]);
        self.data.clear();
        self.data.push(first_channel);

        self.channels = 1;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleFormat {
    U8,
    I16,
    I24,
    I32,
    #[default]
    F32,
}

impl SampleFormat {
    pub fn bits_per_sample(&self) -> u16 {
        match self {
            SampleFormat::U8 => 8,
            SampleFormat::I16 => 16,
            SampleFormat::I24 => 24,
            SampleFormat::I32 => 32,
            SampleFormat::F32 => 32,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Metadata {
    map: std::collections::HashMap<String, String>, // Key-value pairs for metadata fields
    images: Vec<ImageChunk>,                        // Associated images (album art, etc.)
}

impl Metadata {
    pub fn new() -> Self {
        Metadata {
            map: std::collections::HashMap::new(),
            images: Vec::new(),
        }
    }

    pub fn from(chunks: &[MetadataChunk]) -> Self {
        let mut map = std::collections::HashMap::new();
        let mut images = Vec::new();
        for chunk in chunks {
            match chunk {
                MetadataChunk::Picture(image) => {
                    images.push(image.clone());
                }
                _ => {
                    let _ = chunk.parse(&mut map);
                }
            }
        }
        Self { map, images }
    }

    pub fn set_field(&mut self, key: &str, value: &str) -> R<()> {
        self.map.insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub fn get_field(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }

    pub fn add_image(&mut self, image: ImageChunk) {
        self.images.push(image);
    }

    pub fn get_images(&self) -> &[ImageChunk] {
        &self.images
    }
}

// Common metadata parsing utilities
impl Metadata {
    /// Parse iXML metadata from XML string
    pub fn parse_ixml(&mut self, xml: &str) -> R<()> {
        use quick_xml::{Reader, events::Event};

        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut buf = Vec::new();
        let mut current_path = Vec::new();
        let mut current_text = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    current_path.push(name.clone());
                    current_text.clear();
                }
                Ok(Event::End(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    if let Some(last) = current_path.last() {
                        if last == &name && !current_text.trim().is_empty() {
                            // Create a key from the path
                            let key = if current_path.len() > 1 {
                                format!("{}_{}", current_path[current_path.len() - 2], name)
                            } else {
                                name.clone()
                            };
                            self.set_field(&key, current_text.trim())?;
                        }
                    }
                    current_path.pop();
                    current_text.clear();
                }
                Ok(Event::Text(ref e)) => {
                    if let Ok(text) = e.unescape() {
                        current_text.push_str(&text);
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => {
                    // If XML parsing fails, fall back to simple parsing
                    for line in xml.lines() {
                        if let Some(idx) = line.find('=') {
                            let key = line[0..idx].trim();
                            let value = line[idx + 1..].trim();
                            self.set_field(key, value)?;
                        }
                    }
                    break;
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(())
    }

    /// Parse basic ID3 metadata (ID3v1 and simple ID3v2)
    pub fn parse_id3(&mut self, data: &[u8]) -> R<()> {
        if data.is_empty() {
            return Ok(());
        }

        // Check for ID3v2 tag (starts with "ID3")
        if data.len() >= 10 && &data[0..3] == b"ID3" {
            let version_major = data[3];
            let version_minor = data[4];
            
            // Parse syncsafe integer (7 bits per byte, MSB is always 0)
            let size = ((data[6] as u32) << 21)
                | ((data[7] as u32) << 14)
                | ((data[8] as u32) << 7)
                | (data[9] as u32);

            self.set_field("ID3Version", &format!("2.{}.{}", version_major, version_minor))?;

            if size > 0 && data.len() >= (10 + size as usize) {
                let tag_data = &data[10..(10 + size as usize)];
                
                // Simple ID3 parsing - just extract common text frames
                if version_major >= 3 {
                    self.parse_id3v2_frames(tag_data)?;
                }
            }
        }
        // Check for ID3v1 tag (last 128 bytes, starts with "TAG")
        else if data.len() >= 128 && &data[data.len() - 128..data.len() - 125] == b"TAG" {
            let tag_start = data.len() - 128;

            // Parse ID3v1 fields
            if let Some(title) = clean_text_field(&data[tag_start + 3..tag_start + 33]) {
                self.set_field("Title", &title)?;
            }
            if let Some(artist) = clean_text_field(&data[tag_start + 33..tag_start + 63]) {
                self.set_field("Artist", &artist)?;
            }
            if let Some(album) = clean_text_field(&data[tag_start + 63..tag_start + 93]) {
                self.set_field("Album", &album)?;
            }
            if let Some(year) = clean_text_field(&data[tag_start + 93..tag_start + 97]) {
                self.set_field("Year", &year)?;
            }

            self.set_field("ID3Version", "1.0")?;
        }

        Ok(())
    }

    /// Parse ID3v2 frames (basic implementation)
    fn parse_id3v2_frames(&mut self, data: &[u8]) -> R<()> {
        let mut offset = 0;
        while offset + 10 <= data.len() {
            // ID3v2.3/2.4 frame header: 4-byte ID + 4-byte size + 2-byte flags
            let frame_id = String::from_utf8_lossy(&data[offset..offset + 4]).to_string();
            
            let frame_size = ((data[offset + 4] as usize) << 24)
                | ((data[offset + 5] as usize) << 16)
                | ((data[offset + 6] as usize) << 8)
                | (data[offset + 7] as usize);

            if frame_size == 0 || offset + 10 + frame_size > data.len() {
                break;
            }

            let frame_data = &data[offset + 10..offset + 10 + frame_size];

            // Parse common text frames
            if let Some(key) = get_id3_frame_name(&frame_id) {
                if let Some(text) = parse_id3_text_frame(frame_data) {
                    self.set_field(&key, &text)?;
                }
            }

            offset += 10 + frame_size;
        }
        Ok(())
    }

    /// Parse BEXT (BWF) metadata chunk
    pub fn parse_bext(&mut self, data: &[u8]) -> R<()> {
        if data.len() < 602 {
            return Ok(());
        }

        // Description: 256 bytes, null-terminated string
        if let Some(description) = clean_text_field(&data[0..256]) {
            self.set_field("Description", &description)?;
        }

        // Originator: 32 bytes, null-terminated string
        if data.len() >= 288 {
            if let Some(originator) = clean_text_field(&data[256..288]) {
                self.set_field("Originator", &originator)?;
            }
        }

        // OriginatorReference: 32 bytes, null-terminated string
        if data.len() >= 320 {
            if let Some(orig_ref) = clean_text_field(&data[288..320]) {
                self.set_field("OriginatorReference", &orig_ref)?;
            }
        }

        // OriginationDate: 10 bytes, format YYYY-MM-DD
        if data.len() >= 330 {
            if let Some(date) = clean_text_field(&data[320..330]) {
                self.set_field("OriginationDate", &date)?;
            }
        }

        // OriginationTime: 8 bytes, format HH:MM:SS
        if data.len() >= 338 {
            if let Some(time) = clean_text_field(&data[330..338]) {
                self.set_field("OriginationTime", &time)?;
            }
        }

        // TimeReference: 8 bytes, 64-bit integer (little-endian)
        if data.len() >= 346 {
            let time_ref = u64::from_le_bytes([
                data[338], data[339], data[340], data[341], data[342], data[343],
                data[344], data[345],
            ]);
            self.set_field("TimeReference", &time_ref.to_string())?;
        }

        // CodingHistory: remaining bytes, null-terminated string
        if data.len() > 602 {
            if let Some(coding_history) = clean_text_field(&data[602..]) {
                self.set_field("CodingHistory", &coding_history)?;
            }
        }

        Ok(())
    }
}

// Helper functions for common text processing
fn clean_text_field(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn get_id3_frame_name(frame_id: &str) -> Option<String> {
    match frame_id {
        "TIT2" => Some("Title".to_string()),
        "TPE1" => Some("Artist".to_string()),
        "TALB" => Some("Album".to_string()),
        "TYER" | "TDRC" => Some("Year".to_string()),
        "TCON" => Some("Genre".to_string()),
        "TRCK" => Some("Track".to_string()),
        "COMM" => Some("Comment".to_string()),
        "TPE2" => Some("AlbumArtist".to_string()),
        "TCOM" => Some("Composer".to_string()),
        _ => None,
    }
}

fn parse_id3_text_frame(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    // First byte is text encoding, skip it for simplicity
    let text_data = &data[1..];
    let text = String::from_utf8_lossy(text_data).to_string();
    let trimmed = text.trim_end_matches('\0').trim();
    
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct ImageChunk {
    mime_type: String,
    description: String,
    data: Vec<u8>,
}
