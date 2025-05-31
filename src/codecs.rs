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
    pub fn parse(&self) -> R<()> {
        // Silent parsing - debug prints removed for performance
        Ok(())
    }

    pub fn as_text_tags(&self) -> Vec<(String, String)> {
        match self {
            Self::IXml(xml) => {
                let mut tags = Vec::new();
                for line in xml.lines() {
                    if let Some(idx) = line.find('=') {
                        let key = line[0..idx].trim().to_string();
                        let value = line[idx + 1..].trim().to_string();
                        tags.push((key, value));
                    }
                }
                tags
            }
            Self::TextTag { key, value } => {
                vec![(key.clone(), value.clone())]
            }
            _ => Vec::new(),
        }
    }

    pub fn to_format(&self, format: &str) -> Option<MetadataChunk> {
        match (self, format) {
            (Self::IXml(_), "mp3") => {
                let _ = self.as_text_tags();
                Some(Self::ID3(Vec::new()))
            }
            (Self::ID3(_), "wav" | "flac") => Some(Self::IXml(String::new())),
            (
                Self::Picture {
                    mime_type,
                    description,
                    data,
                },
                _,
            ) => Some(Self::Picture {
                mime_type: mime_type.clone(),
                description: description.clone(),
                data: data.clone(),
            }),
            _ => None,
        }
    }

    pub fn new_text_tag(key: &str, value: &str) -> Self {
        Self::TextTag {
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    pub fn new_picture(mime_type: &str, description: &str, data: &[u8]) -> Self {
        Self::Picture {
            mime_type: mime_type.to_string(),
            description: description.to_string(),
            data: data.to_vec(),
        }
    }
}
