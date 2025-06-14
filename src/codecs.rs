use std::collections::HashMap;

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
            return Ok(());
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

#[derive(Debug, Clone)]
pub enum MetadataChunk {
    Bext(Vec<u8>),
    IXml(String),
    Soundminer(Vec<u8>), // could later parse this if needed
    ID3(Vec<u8>),        // For MP3 and potentially other formats
    APE(Vec<u8>),        // APE tags used in various formats
    Picture {
        // For album art/images
        mime_type: String,
        description: String,
        data: Vec<u8>,
    },
    TextTag {
        // For simple text-based metadata
        key: String,
        value: String,
    },
    Unknown {
        id: String,
        data: Vec<u8>,
    },
}

impl MetadataChunk {
    pub fn id(&self) -> String {
        match self {
            MetadataChunk::Bext(_) => "bext".to_string(),
            MetadataChunk::IXml(_) => "iXML".to_string(),
            MetadataChunk::Soundminer(_) => "SMED".to_string(),
            MetadataChunk::ID3(_) => "ID3".to_string(),
            MetadataChunk::APE(_) => "APE".to_string(),
            MetadataChunk::Picture { .. } => "Picture".to_string(),
            MetadataChunk::TextTag { key, .. } => key.clone(),
            MetadataChunk::Unknown { id, .. } => id.clone(),
        }
    }
    pub fn data(&self) -> &[u8] {
        match self {
            MetadataChunk::Bext(data) => data,
            MetadataChunk::IXml(data) => data.as_bytes(),
            MetadataChunk::Soundminer(data) => data,
            MetadataChunk::ID3(data) => data,
            MetadataChunk::APE(data) => data,
            MetadataChunk::Picture { data, .. } => data,
            MetadataChunk::TextTag { value, .. } => value.as_bytes(),
            MetadataChunk::Unknown { data, .. } => data,
        }
    }
    pub fn parse(&self) -> R<HashMap<String, String>> {
        match self {
            Self::Bext(data) => {
                // Parse BEXT metadata according to BWF specification
                let mut map = HashMap::new();

                if data.len() < 602 {
                    // Minimum BEXT chunk size
                    return Ok(map);
                }

                // Description: 256 bytes, null-terminated string
                if data.len() >= 256 {
                    let description = String::from_utf8_lossy(&data[0..256])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !description.is_empty() {
                        map.insert("Description".to_string(), description);
                    }
                }

                // Originator: 32 bytes, null-terminated string
                if data.len() >= 288 {
                    let originator = String::from_utf8_lossy(&data[256..288])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !originator.is_empty() {
                        map.insert("Originator".to_string(), originator);
                    }
                }

                // OriginatorReference: 32 bytes, null-terminated string
                if data.len() >= 320 {
                    let orig_ref = String::from_utf8_lossy(&data[288..320])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !orig_ref.is_empty() {
                        map.insert("OriginatorReference".to_string(), orig_ref);
                    }
                }

                // OriginationDate: 10 bytes, format YYYY-MM-DD
                if data.len() >= 330 {
                    let date = String::from_utf8_lossy(&data[320..330])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !date.is_empty() {
                        map.insert("OriginationDate".to_string(), date);
                    }
                }

                // OriginationTime: 8 bytes, format HH:MM:SS
                if data.len() >= 338 {
                    let time = String::from_utf8_lossy(&data[330..338])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !time.is_empty() {
                        map.insert("OriginationTime".to_string(), time);
                    }
                }

                // TimeReference: 8 bytes, 64-bit integer (little-endian)
                if data.len() >= 346 {
                    let time_ref = u64::from_le_bytes([
                        data[338], data[339], data[340], data[341], data[342], data[343],
                        data[344], data[345],
                    ]);
                    map.insert("TimeReference".to_string(), time_ref.to_string());
                }

                // Version: 2 bytes, little-endian
                if data.len() >= 348 {
                    let version = u16::from_le_bytes([data[346], data[347]]);
                    map.insert("Version".to_string(), version.to_string());
                }

                // UMID: 64 bytes
                if data.len() >= 412 {
                    let mut umid = String::with_capacity(128); // 64 bytes * 2 hex chars
                    for &b in &data[348..412] {
                        umid.push_str(&format!("{:02X}", b));
                    }
                    if umid.chars().any(|c| c != '0') {
                        // Only include if not all zeros
                        map.insert("UMID".to_string(), umid);
                    }
                }

                // LoudnessValue: 2 bytes, little-endian (signed)
                if data.len() >= 414 {
                    let loudness = i16::from_le_bytes([data[412], data[413]]);
                    if loudness != 0 {
                        // Only include if not zero (which means not set)
                        map.insert("LoudnessValue".to_string(), loudness.to_string());
                    }
                }

                // LoudnessRange: 2 bytes, little-endian
                if data.len() >= 416 {
                    let loudness_range = u16::from_le_bytes([data[414], data[415]]);
                    if loudness_range != 0 {
                        map.insert("LoudnessRange".to_string(), loudness_range.to_string());
                    }
                }

                // MaxTruePeakLevel: 2 bytes, little-endian (signed)
                if data.len() >= 418 {
                    let max_peak = i16::from_le_bytes([data[416], data[417]]);
                    if max_peak != 0 {
                        map.insert("MaxTruePeakLevel".to_string(), max_peak.to_string());
                    }
                }

                // MaxMomentaryLoudness: 2 bytes, little-endian (signed)
                if data.len() >= 420 {
                    let max_momentary = i16::from_le_bytes([data[418], data[419]]);
                    if max_momentary != 0 {
                        map.insert(
                            "MaxMomentaryLoudness".to_string(),
                            max_momentary.to_string(),
                        );
                    }
                }

                // MaxShortTermLoudness: 2 bytes, little-endian (signed)
                if data.len() >= 422 {
                    let max_short_term = i16::from_le_bytes([data[420], data[421]]);
                    if max_short_term != 0 {
                        map.insert(
                            "MaxShortTermLoudness".to_string(),
                            max_short_term.to_string(),
                        );
                    }
                }

                // Reserved: 180 bytes (skip)

                // CodingHistory: remaining bytes, null-terminated string
                if data.len() > 602 {
                    let coding_history = String::from_utf8_lossy(&data[602..])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !coding_history.is_empty() {
                        map.insert("CodingHistory".to_string(), coding_history);
                    }
                }

                Ok(map)
            }
            Self::IXml(xml) => {
                // Parse iXML metadata using proper XML parsing
                use quick_xml::{Reader, events::Event};

                let mut map = HashMap::new();
                let mut reader = Reader::from_str(xml);
                reader.config_mut().trim_text(true);

                let mut buf = Vec::new();
                let mut current_path = Vec::new();
                let mut current_text = String::new();

                loop {
                    match reader.read_event_into(&mut buf) {
                        Ok(Event::Start(ref e)) => {
                            let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                            current_path.push(name);
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
                                    map.insert(key, current_text.trim().to_string());
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
                                    let key = line[0..idx].trim().to_string();
                                    let value = line[idx + 1..].trim().to_string();
                                    map.insert(key, value);
                                }
                            }
                            break;
                        }
                        _ => {}
                    }
                    buf.clear();
                }

                Ok(map)
            }

            Self::ID3(data) => {
                // Parse ID3 tags (supports ID3v1 and ID3v2.x)
                let mut map = HashMap::new();

                if data.is_empty() {
                    return Ok(map);
                }

                // Check for ID3v2 tag (starts with "ID3")
                if data.len() >= 10 && &data[0..3] == b"ID3" {
                    // ID3v2 header: "ID3" + version (2 bytes) + flags (1 byte) + size (4 bytes syncsafe)
                    let version_major = data[3];
                    let version_minor = data[4];
                    let _flags = data[5]; // Flags byte (currently unused)

                    // Parse syncsafe integer (7 bits per byte, MSB is always 0)
                    let size = ((data[6] as u32) << 21)
                        | ((data[7] as u32) << 14)
                        | ((data[8] as u32) << 7)
                        | (data[9] as u32);

                    map.insert(
                        "ID3Version".to_string(),
                        format!("2.{}.{}", version_major, version_minor),
                    );

                    if size > 0 && data.len() >= (10 + size as usize) {
                        let tag_data = &data[10..(10 + size as usize)];

                        // Parse frames based on version
                        match version_major {
                            2 => self.parse_id3v22_frames(tag_data, &mut map),
                            3 | 4 => {
                                self.parse_id3v23_v24_frames(tag_data, &mut map, version_major)
                            }
                            _ => {} // Unsupported version
                        }
                    }
                }
                // Check for ID3v1 tag (last 128 bytes, starts with "TAG")
                else if data.len() >= 128 && &data[data.len() - 128..data.len() - 125] == b"TAG" {
                    let tag_start = data.len() - 128;

                    // Title: 30 bytes
                    let title = String::from_utf8_lossy(&data[tag_start + 3..tag_start + 33])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !title.is_empty() {
                        map.insert("Title".to_string(), title);
                    }

                    // Artist: 30 bytes
                    let artist = String::from_utf8_lossy(&data[tag_start + 33..tag_start + 63])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !artist.is_empty() {
                        map.insert("Artist".to_string(), artist);
                    }

                    // Album: 30 bytes
                    let album = String::from_utf8_lossy(&data[tag_start + 63..tag_start + 93])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !album.is_empty() {
                        map.insert("Album".to_string(), album);
                    }

                    // Year: 4 bytes
                    let year = String::from_utf8_lossy(&data[tag_start + 93..tag_start + 97])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !year.is_empty() {
                        map.insert("Year".to_string(), year);
                    }

                    // Comment: 28 or 30 bytes (depends on if track number is present)
                    let comment_end = if data[tag_start + 125] == 0 && data[tag_start + 126] != 0 {
                        // ID3v1.1 with track number
                        let track = data[tag_start + 126];
                        if track != 0 {
                            map.insert("Track".to_string(), track.to_string());
                        }
                        tag_start + 125
                    } else {
                        // ID3v1 without track number
                        tag_start + 127
                    };

                    let comment = String::from_utf8_lossy(&data[tag_start + 97..comment_end])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    if !comment.is_empty() {
                        map.insert("Comment".to_string(), comment);
                    }

                    // Genre: 1 byte (index into predefined list)
                    let genre_byte = data[tag_start + 127];
                    if let Some(genre) = self.get_id3v1_genre(genre_byte) {
                        map.insert("Genre".to_string(), genre);
                    }

                    map.insert("ID3Version".to_string(), "1.0".to_string());
                }

                Ok(map)
            }
            _ => Ok(HashMap::new()),
            // Self::Soundminer(data) => {
            //     // Soundminer parsing logic (if needed)
            //     Ok(HashMap::new())
            // }
            // Self::ID3(data) => {
            //     // ID3 parsing logic (if needed)
            //     Ok(HashMap::new())
            // }
            // Self::APE(data) => {
            //     // APE parsing logic (if needed)
            //     Ok(HashMap::new())
            // }
            // Self::Picture { .. } => Ok(HashMap::new()),
            // Self::TextTag { key, value } => {
            //     let mut map = HashMap::new();
            //     map.insert(key.clone(), value.clone());
            //     Ok(map)
            // }
            // Self::Unknown { id, data } => {
            //     let mut map = HashMap::new();
            //     map.insert(id.clone(), String::from_utf8_lossy(data).to_string());
            //     Ok(map)
            // }
        }
    }

    // Helper methods for ID3 parsing
    fn parse_id3v22_frames(&self, data: &[u8], map: &mut HashMap<String, String>) {
        let mut offset = 0;
        while offset + 6 <= data.len() {
            // ID3v2.2 frame header: 3-byte ID + 3-byte size
            let frame_id = String::from_utf8_lossy(&data[offset..offset + 3]).to_string();
            let frame_size = ((data[offset + 3] as usize) << 16)
                | ((data[offset + 4] as usize) << 8)
                | (data[offset + 5] as usize);

            if frame_size == 0 || offset + 6 + frame_size > data.len() {
                break;
            }

            let frame_data = &data[offset + 6..offset + 6 + frame_size];

            // Parse common text frames
            if let Some(key) = self.get_id3v22_frame_name(&frame_id) {
                if let Some(text) = self.parse_text_frame(frame_data) {
                    map.insert(key, text);
                }
            }

            offset += 6 + frame_size;
        }
    }

    fn parse_id3v23_v24_frames(&self, data: &[u8], map: &mut HashMap<String, String>, version: u8) {
        let mut offset = 0;
        while offset + 10 <= data.len() {
            // ID3v2.3/2.4 frame header: 4-byte ID + 4-byte size + 2-byte flags
            let frame_id = String::from_utf8_lossy(&data[offset..offset + 4]).to_string();

            let frame_size = if version == 4 {
                // ID3v2.4 uses syncsafe integers for frame size
                ((data[offset + 4] as usize) << 21)
                    | ((data[offset + 5] as usize) << 14)
                    | ((data[offset + 6] as usize) << 7)
                    | (data[offset + 7] as usize)
            } else {
                // ID3v2.3 uses regular integers
                ((data[offset + 4] as usize) << 24)
                    | ((data[offset + 5] as usize) << 16)
                    | ((data[offset + 6] as usize) << 8)
                    | (data[offset + 7] as usize)
            };

            if frame_size == 0 || offset + 10 + frame_size > data.len() {
                break;
            }

            let frame_data = &data[offset + 10..offset + 10 + frame_size];

            // Parse common text frames
            if let Some(key) = self.get_id3v23_v24_frame_name(&frame_id) {
                if let Some(text) = self.parse_text_frame(frame_data) {
                    map.insert(key, text);
                }
            }

            offset += 10 + frame_size;
        }
    }

    fn parse_text_frame(&self, data: &[u8]) -> Option<String> {
        if data.is_empty() {
            return None;
        }

        // First byte is text encoding
        let encoding = data[0];
        let text_data = &data[1..];

        let text = match encoding {
            0 => {
                // ISO-8859-1
                String::from_utf8_lossy(text_data).to_string()
            }
            1 => {
                // UTF-16 with BOM
                if text_data.len() >= 2 {
                    let bom = u16::from_be_bytes([text_data[0], text_data[1]]);
                    let (text_bytes, is_be) = if bom == 0xFEFF {
                        (&text_data[2..], true)
                    } else if bom == 0xFFFE {
                        (&text_data[2..], false)
                    } else {
                        (text_data, true) // Default to big-endian
                    };

                    self.decode_utf16(text_bytes, is_be)
                } else {
                    String::new()
                }
            }
            2 => {
                // UTF-16BE without BOM
                self.decode_utf16(text_data, true)
            }
            3 => {
                // UTF-8
                String::from_utf8_lossy(text_data).to_string()
            }
            _ => {
                // Unknown encoding, try UTF-8
                String::from_utf8_lossy(text_data).to_string()
            }
        };

        let trimmed = text.trim_end_matches('\0').trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn decode_utf16(&self, data: &[u8], big_endian: bool) -> String {
        let mut result = String::new();
        let mut i = 0;

        while i + 1 < data.len() {
            let code_unit = if big_endian {
                u16::from_be_bytes([data[i], data[i + 1]])
            } else {
                u16::from_le_bytes([data[i], data[i + 1]])
            };

            if code_unit == 0 {
                break; // Null terminator
            }

            // Handle surrogate pairs for characters outside BMP
            if (0xD800..=0xDBFF).contains(&code_unit) && i + 3 < data.len() {
                let low_surrogate = if big_endian {
                    u16::from_be_bytes([data[i + 2], data[i + 3]])
                } else {
                    u16::from_le_bytes([data[i + 2], data[i + 3]])
                };

                if (0xDC00..=0xDFFF).contains(&low_surrogate) {
                    let code_point = 0x10000u32
                        + (((code_unit as u32) & 0x3FF) << 10)
                        + ((low_surrogate as u32) & 0x3FF);
                    if let Some(ch) = char::from_u32(code_point) {
                        result.push(ch);
                    }
                    i += 4;
                    continue;
                }
            }

            if let Some(ch) = char::from_u32(code_unit as u32) {
                result.push(ch);
            }
            i += 2;
        }

        result
    }

    fn get_id3v22_frame_name(&self, frame_id: &str) -> Option<String> {
        match frame_id {
            "TT2" => Some("Title".to_string()),
            "TP1" => Some("Artist".to_string()),
            "TAL" => Some("Album".to_string()),
            "TYE" => Some("Year".to_string()),
            "TCO" => Some("Genre".to_string()),
            "TRK" => Some("Track".to_string()),
            "COM" => Some("Comment".to_string()),
            "TP2" => Some("AlbumArtist".to_string()),
            "TT1" => Some("ContentGroup".to_string()),
            "TT3" => Some("Subtitle".to_string()),
            "TP3" => Some("Conductor".to_string()),
            "TP4" => Some("ModifiedBy".to_string()),
            "TCM" => Some("Composer".to_string()),
            _ => None,
        }
    }

    fn get_id3v23_v24_frame_name(&self, frame_id: &str) -> Option<String> {
        match frame_id {
            "TIT2" => Some("Title".to_string()),
            "TPE1" => Some("Artist".to_string()),
            "TALB" => Some("Album".to_string()),
            "TYER" | "TDRC" => Some("Year".to_string()), // TYER in v2.3, TDRC in v2.4
            "TCON" => Some("Genre".to_string()),
            "TRCK" => Some("Track".to_string()),
            "COMM" => Some("Comment".to_string()),
            "TPE2" => Some("AlbumArtist".to_string()),
            "TIT1" => Some("ContentGroup".to_string()),
            "TIT3" => Some("Subtitle".to_string()),
            "TPE3" => Some("Conductor".to_string()),
            "TPE4" => Some("ModifiedBy".to_string()),
            "TCOM" => Some("Composer".to_string()),
            "TPOS" => Some("DiscNumber".to_string()),
            "TBPM" => Some("BPM".to_string()),
            "TKEY" => Some("InitialKey".to_string()),
            "TLAN" => Some("Language".to_string()),
            "TLEN" => Some("Length".to_string()),
            "TMED" => Some("MediaType".to_string()),
            "TOAL" => Some("OriginalAlbum".to_string()),
            "TOFN" => Some("OriginalFilename".to_string()),
            "TOLY" => Some("OriginalLyricist".to_string()),
            "TOPE" => Some("OriginalArtist".to_string()),
            "TORY" => Some("OriginalYear".to_string()),
            "TOWN" => Some("FileOwner".to_string()),
            "TPUB" => Some("Publisher".to_string()),
            "TRDA" => Some("RecordingDates".to_string()),
            "TRSN" => Some("InternetRadioName".to_string()),
            "TRSO" => Some("InternetRadioOwner".to_string()),
            "TSIZ" => Some("Size".to_string()),
            "TSRC" => Some("ISRC".to_string()),
            "TSSE" => Some("EncodingSettings".to_string()),
            _ => None,
        }
    }

    fn get_id3v1_genre(&self, genre_byte: u8) -> Option<String> {
        let genres = [
            "Blues",
            "Classic Rock",
            "Country",
            "Dance",
            "Disco",
            "Funk",
            "Grunge",
            "Hip-Hop",
            "Jazz",
            "Metal",
            "New Age",
            "Oldies",
            "Other",
            "Pop",
            "R&B",
            "Rap",
            "Reggae",
            "Rock",
            "Techno",
            "Industrial",
            "Alternative",
            "Ska",
            "Death Metal",
            "Pranks",
            "Soundtrack",
            "Euro-Techno",
            "Ambient",
            "Trip-Hop",
            "Vocal",
            "Jazz+Funk",
            "Fusion",
            "Trance",
            "Classical",
            "Instrumental",
            "Acid",
            "House",
            "Game",
            "Sound Clip",
            "Gospel",
            "Noise",
            "Alternative Rock",
            "Bass",
            "Soul",
            "Punk",
            "Space",
            "Meditative",
            "Instrumental Pop",
            "Instrumental Rock",
            "Ethnic",
            "Gothic",
            "Darkwave",
            "Techno-Industrial",
            "Electronic",
            "Pop-Folk",
            "Eurodance",
            "Dream",
            "Southern Rock",
            "Comedy",
            "Cult",
            "Gangsta",
            "Top 40",
            "Christian Rap",
            "Pop/Funk",
            "Jungle",
            "Native US",
            "Cabaret",
            "New Wave",
            "Psychadelic",
            "Rave",
            "Showtunes",
            "Trailer",
            "Lo-Fi",
            "Tribal",
            "Acid Punk",
            "Acid Jazz",
            "Polka",
            "Retro",
            "Musical",
            "Rock & Roll",
            "Hard Rock",
        ];

        if (genre_byte as usize) < genres.len() {
            Some(genres[genre_byte as usize].to_string())
        } else {
            None
        }
    }
}
