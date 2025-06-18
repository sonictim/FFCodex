use crate::prelude::*;

use minimp3::{Decoder as Mp3Decoder, Frame};

pub struct Mp3Codec;

impl Codec for Mp3Codec {
    fn file_extension(&self) -> &'static str {
        "mp3"
    }

    fn validate_file_format(&self, data: &[u8]) -> R<()> {
        if data.len() < 3 || &data[0..3] != b"ID3" {
            return Err(anyhow!("Invalid MP3 file: Missing ID3 tag"));
        }
        Ok(())
    }

    fn get_file_info(&self, file_path: &str) -> R<FileInfo> {
        use std::fs;
        use memmap2::MmapOptions;

        let file = fs::File::open(file_path)?;
        let file_size = file.metadata()?.len() as usize;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        self.validate_file_format(&mapped_file)?;

        // Look for ID3v2 tag at the beginning for description
        let mut description = String::new();
        if mapped_file.len() >= 10 && &mapped_file[0..3] == b"ID3" {
            // ID3v2 header: ID3 + version (2 bytes) + flags (1 byte) + size (4 bytes)
            let size = ((mapped_file[6] as u32) << 21) |
                      ((mapped_file[7] as u32) << 14) |
                      ((mapped_file[8] as u32) << 7) |
                      (mapped_file[9] as u32);
            
            if size > 0 && size < mapped_file.len() as u32 - 10 {
                let id3_data = &mapped_file[10..(10 + size as usize)];
                description = extract_id3_description(id3_data);
            }
        }

        // If no ID3v2, check for ID3v1 at the end
        if description.is_empty() && mapped_file.len() >= 128 {
            let id3v1_start = mapped_file.len() - 128;
            if &mapped_file[id3v1_start..id3v1_start + 3] == b"TAG" {
                // ID3v1 comment field is at offset 97-126 (30 bytes)
                let comment_start = id3v1_start + 97;
                let comment_end = id3v1_start + 127;
                let comment_data = &mapped_file[comment_start..comment_end];
                description = String::from_utf8_lossy(comment_data)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
            }
        }

        // Use MP3 decoder to extract basic information from the first frame
        let mut decoder = Mp3Decoder::new(Cursor::new(&mapped_file[..]));

        let mut sample_rate = 0;
        let mut channels = 0;
        let mut total_samples = 0;

        // Read frames to get format information and estimate duration
        while let Ok(Frame {
            data,
            sample_rate: sr,
            channels: ch,
            ..
        }) = decoder.next_frame()
        {
            if sample_rate == 0 {
                sample_rate = sr;
                channels = ch;
            }
            total_samples += data.len() / ch;
        }

        if sample_rate == 0 || channels == 0 {
            return Err(anyhow!("Could not determine MP3 format information"));
        }

        // Calculate duration (this is an approximation since we counted samples)
        let duration_seconds = if sample_rate > 0 {
            total_samples as f64 / (sample_rate * channels) as f64
        } else {
            0.0
        };

        let duration = if duration_seconds >= 3600.0 {
            format!(
                "{:.0}:{:02.0}:{:02.0}",
                duration_seconds / 3600.0,
                (duration_seconds % 3600.0) / 60.0,
                duration_seconds % 60.0
            )
        } else {
            format!("{:.0}:{:02.0}", duration_seconds / 60.0, duration_seconds % 60.0)
        };

        Ok(FileInfo {
            path: file_path.to_string(),
            size: file_size,
            sample_rate: sample_rate as u16,
            channels: channels as u16,
            bit_depth: 16, // MP3 is typically decoded to 16-bit
            duration,
            description,
        })
    }

    fn decode(&self, input: &[u8]) -> R<AudioBuffer> {
        self.validate_file_format(input)?;

        let mut decoder = Mp3Decoder::new(Cursor::new(input));
        let mut audio_data: Vec<Vec<f32>> = Vec::new();
        let mut sample_rate = 0;
        let mut channels = 0;

        while let Ok(Frame {
            data,
            sample_rate: sr,
            channels: ch,
            ..
        }) = decoder.next_frame()
        {
            sample_rate = sr;
            channels = ch;

            if audio_data.is_empty() {
                audio_data = vec![Vec::new(); channels];
            }

            data.chunks_exact(channels).for_each(|chunk| {
                for (i, &sample) in chunk.iter().enumerate() {
                    audio_data[i].push(sample as f32 / i16::MAX as f32);
                }
            });
        }

        Ok(AudioBuffer {
            sample_rate: sample_rate as u32,
            channels: channels as u16,
            format: SampleFormat::F32,
            data: audio_data,
        })
    }

    fn encode(&self, buffer: &AudioBuffer) -> R<Vec<u8>> {
        // Validate input buffer
        if buffer.data.is_empty() {
            return Err(anyhow!("Empty audio buffer"));
        }

        let channels = buffer.channels as usize;
        if channels == 0 || channels > 2 {
            return Err(anyhow!(
                "MP3 encoding only supports mono or stereo (got {} channels)",
                channels
            ));
        }

        if buffer.data.len() != channels {
            return Err(anyhow!(
                "Buffer channel count ({}) doesn't match channel data length ({})",
                channels,
                buffer.data.len()
            ));
        }

        let mut output = Vec::new();
        let mut lame =
            lame::Lame::new().ok_or_else(|| anyhow!("Failed to initialize LAME encoder"))?;

        // Configure encoder
        lame.set_sample_rate(buffer.sample_rate)
            .map_err(|e| anyhow!("Failed to set sample rate: {:?}", e))?;
        lame.set_channels(buffer.channels as u8)
            .map_err(|e| anyhow!("Failed to set channels: {:?}", e))?;
        lame.set_quality(2)
            .map_err(|e| anyhow!("Failed to set quality: {:?}", e))?; // High quality

        // CRITICAL: Initialize encoder parameters
        lame.init_params()
            .map_err(|e| anyhow!("Failed to initialize encoder parameters: {:?}", e))?;

        // Prepare samples based on channel count
        match buffer.channels {
            1 => {
                // Mono case - convert to i16 samples
                let samples: Vec<i16> = buffer.data[0]
                    .iter()
                    .map(|&sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .collect();

                let mut mp3_buffer = vec![0; samples.len() * 5 / 4 + 7200]; // Buffer size recommendation from LAME docs

                // For mono in lame 0.1.3, we need to pass PCM data as left channel and NULL as right channel
                // The key is that when encoding mono, we should NOT pass an empty array for right channel
                let bytes_written = lame
                    .encode(&samples, &samples, &mut mp3_buffer)
                    .map_err(|e| anyhow!("Lame encoding error: {:?}", e))?;

                mp3_buffer.truncate(bytes_written);
                output.extend_from_slice(&mp3_buffer);

                // Flush any remaining frames
                let mut flush_buffer = vec![0; 7200];
                let empty_buffer: Vec<i16> = Vec::new();
                let bytes_written = lame
                    .encode(&empty_buffer, &empty_buffer, &mut flush_buffer)
                    .map_err(|e| anyhow!("Lame flush error: {:?}", e))?;

                flush_buffer.truncate(bytes_written);
                output.extend_from_slice(&flush_buffer);
            }
            2 => {
                // Stereo case
                let left: Vec<i16> = buffer.data[0]
                    .iter()
                    .map(|&sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .collect();

                let right: Vec<i16> = buffer.data[1]
                    .iter()
                    .map(|&sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .collect();

                let mut mp3_buffer = vec![0; left.len() * 5 / 2 + 7200]; // Buffer size for stereo
                let bytes_written = lame
                    .encode(&left, &right, &mut mp3_buffer)
                    .map_err(|e| anyhow!("Lame encoding error: {:?}", e))?;

                mp3_buffer.truncate(bytes_written);
                output.extend_from_slice(&mp3_buffer);

                // Flush any remaining frames
                let mut flush_buffer = vec![0; 7200];
                let empty_buffer: Vec<i16> = Vec::new();
                let bytes_written = lame
                    .encode(&empty_buffer, &empty_buffer, &mut flush_buffer)
                    .map_err(|e| anyhow!("Lame flush error: {:?}", e))?;

                flush_buffer.truncate(bytes_written);
                output.extend_from_slice(&flush_buffer);
            }
            _ => unreachable!(), // We've already validated the channel count
        }

        Ok(output)
    }

    fn embed_metadata_chunks(&self, _input: &[u8], _chunks: &[MetadataChunk]) -> R<Vec<u8>> {
        todo!()
    }

    fn extract_metadata_chunks(&self, _input: &[u8]) -> R<Vec<MetadataChunk>> {
        todo!()
    }
    fn embed_metadata_to_file(&self, _file_path: &str, _metadata: &Metadata) -> R<()> {
        todo!()
    }
    fn extract_metadata_from_file(&self, _file_path: &str) -> R<Metadata> {
        todo!()
    }
}

// Helper function to extract description from ID3v2 data
fn extract_id3_description(id3_data: &[u8]) -> String {
    let mut offset = 0;
    
    while offset + 10 < id3_data.len() {
        // ID3v2.3/2.4 frame header: frame_id (4 bytes) + size (4 bytes) + flags (2 bytes)
        let frame_id = &id3_data[offset..offset + 4];
        
        // Read frame size (big-endian)
        let frame_size = ((id3_data[offset + 4] as u32) << 24) |
                        ((id3_data[offset + 5] as u32) << 16) |
                        ((id3_data[offset + 6] as u32) << 8) |
                        (id3_data[offset + 7] as u32);
        
        if frame_size == 0 || offset + 10 + frame_size as usize > id3_data.len() {
            break;
        }
        
        // Check for comment frames (COMM) or user text frames (TXXX)
        if frame_id == b"COMM" || frame_id == b"TXXX" {
            let frame_data = &id3_data[offset + 10..offset + 10 + frame_size as usize];
            
            if !frame_data.is_empty() {
                // Skip encoding byte and language (for COMM) or encoding byte (for TXXX)
                let text_start = if frame_id == b"COMM" { 4 } else { 1 };
                
                if frame_data.len() > text_start {
                    let text = String::from_utf8_lossy(&frame_data[text_start..])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string();
                    
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
        }
        
        offset += 10 + frame_size as usize;
    }
    
    String::new()
}
