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
