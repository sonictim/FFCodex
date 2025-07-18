use crate::prelude::*;
use claxon::FlacReader;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use metaflac::{Block, Tag};

// FLAC-specific constants
const FLAC_MARKER: &[u8; 4] = b"fLaC";
const STREAMINFO_BLOCK_TYPE: u8 = 0;
// const SEEKTABLE_BLOCK_TYPE: u8 = 3;
const VORBIS_COMMENT_BLOCK_TYPE: u8 = 4;
// Note: PICTURE_BLOCK_TYPE and LAST_METADATA_BLOCK_FLAG removed as unused

// Sample normalization constants
const I16_MAX_F: f32 = 32767.0;
const I16_DIVISOR: f32 = 32768.0;
const I24_MAX_F: f32 = 8388607.0;
const I24_DIVISOR: f32 = 8388608.0;
const I32_MAX_F: f32 = 2147483647.0;
const I32_DIVISOR: f32 = 2147483648.0;

pub struct FlacCodec;

impl Codec for FlacCodec {
    fn as_str(&self) -> &'static str {
        "FLAC"
    }
    fn file_extension(&self) -> &'static str {
        "flac"
    }

    fn validate_file_format(&self, data: &[u8]) -> R<()> {
        // Check if the file is too small
        if data.len() < 4 {
            return Err(anyhow!("File too small to be a valid FLAC"));
        }

        // Check for 'fLaC' marker at the beginning of the file
        if &data[0..4] != FLAC_MARKER {
            return Err(anyhow!("Not a valid FLAC file: Missing fLaC marker"));
        }

        Ok(())
    }
    fn get_file_info(&self, file_path: &str) -> R<FileInfo> {
        use memmap2::MmapOptions;
        use std::fs;

        let file = fs::File::open(file_path)?;
        let file_size = file.metadata()?.len() as usize;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        self.validate_file_format(&mapped_file)?;

        let mut cursor = Cursor::new(&mapped_file[..]);

        // Skip FLAC marker
        cursor.seek(SeekFrom::Start(4))?;

        let mut sample_rate = 0u32;
        let mut channels = 0u16;
        let mut bits_per_sample = 0u16;
        let mut total_samples = 0u64;
        let mut description = String::new();

        // Read metadata blocks
        loop {
            let header = cursor.read_u8()?;
            let is_last = (header & 0x80) != 0;
            let block_type = header & 0x7F;
            let block_size = cursor.read_u24::<BigEndian>()? as usize;

            match block_type {
                STREAMINFO_BLOCK_TYPE => {
                    if block_size < 34 {
                        return Err(anyhow!("Invalid STREAMINFO block size"));
                    }

                    // Read STREAMINFO data
                    let mut streaminfo = vec![0u8; block_size];
                    cursor.read_exact(&mut streaminfo)?;

                    // Parse STREAMINFO according to FLAC spec
                    sample_rate = ((streaminfo[10] as u32) << 12)
                        | ((streaminfo[11] as u32) << 4)
                        | ((streaminfo[12] as u32) >> 4);

                    channels = (((streaminfo[12] as u16) >> 1) & 0x07) + 1;

                    bits_per_sample = ((((streaminfo[12] as u16) & 0x01) << 4)
                        | ((streaminfo[13] as u16) >> 4))
                        + 1;

                    // Total samples (36-bit value)
                    total_samples = ((streaminfo[13] as u64 & 0x0F) << 32)
                        | ((streaminfo[14] as u64) << 24)
                        | ((streaminfo[15] as u64) << 16)
                        | ((streaminfo[16] as u64) << 8)
                        | (streaminfo[17] as u64);
                }
                VORBIS_COMMENT_BLOCK_TYPE => {
                    // VORBIS_COMMENT block contains metadata including possible description
                    if description.is_empty() {
                        let mut comment_data = vec![0u8; block_size];
                        cursor.read_exact(&mut comment_data)?;

                        let mut comment_cursor = Cursor::new(&comment_data);

                        // Read vendor string length and skip it
                        if let Ok(vendor_length) = comment_cursor.read_u32::<LittleEndian>() {
                            if vendor_length as usize <= comment_data.len() - 4 {
                                comment_cursor.seek(SeekFrom::Current(vendor_length as i64))?;

                                // Read number of comments
                                if let Ok(comment_count) = comment_cursor.read_u32::<LittleEndian>()
                                {
                                    for _ in 0..comment_count {
                                        if let Ok(comment_length) =
                                            comment_cursor.read_u32::<LittleEndian>()
                                        {
                                            if comment_length > 0 && comment_length as usize <= 1024
                                            {
                                                // Reasonable limit
                                                let mut comment =
                                                    vec![0u8; comment_length as usize];
                                                if comment_cursor.read_exact(&mut comment).is_ok() {
                                                    let comment_str =
                                                        String::from_utf8_lossy(&comment);
                                                    if let Some(eq_pos) = comment_str.find('=') {
                                                        let key =
                                                            comment_str[..eq_pos].to_lowercase();
                                                        let value = &comment_str[eq_pos + 1..];

                                                        if (key == "description"
                                                            || key == "comment")
                                                            && !value.trim().is_empty()
                                                        {
                                                            description = value.trim().to_string();
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        cursor.seek(SeekFrom::Current(block_size as i64))?;
                    }
                }
                _ => {
                    // Skip other metadata blocks
                    cursor.seek(SeekFrom::Current(block_size as i64))?;
                }
            }

            if is_last {
                break;
            }
        }

        if sample_rate == 0 || channels == 0 {
            return Err(anyhow!("Invalid FLAC STREAMINFO data"));
        }

        // Calculate duration
        let duration_seconds = if sample_rate > 0 && total_samples > 0 {
            total_samples as f64 / sample_rate as f64
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
            format!(
                "{:.0}:{:02.0}",
                duration_seconds / 60.0,
                duration_seconds % 60.0
            )
        };

        Ok(FileInfo {
            path: file_path.to_string(),
            size: file_size,
            sample_rate: sample_rate as u16,
            channels,
            bit_depth: bits_per_sample,
            duration,
            description,
        })
    }

    fn decode(&self, input: &[u8]) -> R<AudioBuffer> {
        // Use claxon to decode the FLAC file
        let cursor = Cursor::new(input);
        let mut reader = FlacReader::new(cursor)?;

        let streaminfo = reader.streaminfo();
        let sample_rate = streaminfo.sample_rate;
        let channels = streaminfo.channels as u16;
        let bits_per_sample = streaminfo.bits_per_sample as u16;

        // Pre-allocate the entire audio buffer with exact capacity
        let num_samples = streaminfo.samples.unwrap_or(0) as usize;
        let samples_per_channel = if num_samples > 0 {
            num_samples / channels as usize
        } else {
            0
        };

        // Calculate the appropriate divisor once, outside the loop
        let divisor = match bits_per_sample {
            16 => I16_DIVISOR,
            24 => I24_DIVISOR,
            32 => I32_DIVISOR,
            _ => (1 << (bits_per_sample - 1)) as f32,
        };

        let channel_count = channels as usize;
        let mut audio_data: Vec<Vec<f32>> =
            vec![Vec::with_capacity(samples_per_channel); channel_count];

        // Simplified processing: use parallel processing only for very large files
        let use_parallel = samples_per_channel > 100_000 && channel_count > 1;

        if use_parallel {
            // Collect all samples first for parallel processing
            let mut all_samples = Vec::with_capacity(num_samples);
            for sample_result in reader.samples() {
                all_samples.push(sample_result?);
            }

            // Process in parallel chunks
            let chunk_size = (num_samples / rayon::current_num_threads()).max(1024);
            let chunks: Vec<Vec<f32>> = all_samples
                .par_chunks(chunk_size)
                .map(|chunk| {
                    chunk
                        .iter()
                        .map(|&sample| (sample as f32) / divisor)
                        .collect::<Vec<f32>>()
                })
                .collect();

            // Distribute to channels
            for (chunk_idx, chunk) in chunks.iter().enumerate() {
                for (i, &sample) in chunk.iter().enumerate() {
                    let sample_idx = chunk_idx * chunk_size + i;
                    let ch = sample_idx % channel_count;
                    audio_data[ch].push(sample);
                }
            }
        } else {
            // Simple sequential processing
            let mut sample_buffer = Vec::with_capacity(channel_count);
            for sample_result in reader.samples() {
                let sample = sample_result?;
                sample_buffer.push((sample as f32) / divisor);

                if sample_buffer.len() == channel_count {
                    for (ch, &sample) in sample_buffer.iter().enumerate() {
                        audio_data[ch].push(sample);
                    }
                    sample_buffer.clear();
                }
            }
        }

        // Select the appropriate sample format based on bit depth
        let format = select_sample_format(bits_per_sample);

        Ok(AudioBuffer {
            sample_rate,
            channels,
            format,
            data: audio_data,
        })
    }
    fn encode(&self, buffer: &Option<AudioBuffer>) -> R<Vec<u8>> {
        let Some(buffer) = buffer else {
            return Err(anyhow!("Cannot encode None AudioBuffer"));
        };
        // Get audio parameters
        let bits_per_sample = get_bits_per_sample(buffer.format);
        let channels = buffer.channels as usize;
        let sample_rate = buffer.sample_rate as usize;

        if buffer.data.is_empty() || buffer.data[0].is_empty() {
            return Err(anyhow!("Cannot encode empty audio buffer"));
        }

        let num_samples = buffer.data[0].len();

        // Pre-calculate conversion factors outside of the loop for better performance
        let scale_factor = match bits_per_sample {
            8 => 127.0,
            16 => I16_MAX_F,
            24 => I24_MAX_F,
            32 => I32_MAX_F,
            _ => {
                return Err(anyhow!(
                    "Unsupported bit depth for FLAC encoding: {}",
                    bits_per_sample
                ));
            }
        };

        // Create the interleaved samples vector using either parallel or sequential approach
        let interleaved_samples = if num_samples > 100_000 {
            // For large files, use parallel processing with thread-local data
            let chunk_size = (num_samples / rayon::current_num_threads()).max(1024);

            // Use parallel iterator with collect to build the final vector
            (0..num_samples)
                .into_par_iter()
                .chunks(chunk_size)
                .flat_map(|chunk_indices| {
                    // Create a local buffer for each thread
                    let mut local_buffer = Vec::with_capacity(chunk_indices.len() * channels);

                    // Process samples in this chunk
                    for i in chunk_indices {
                        for ch in 0..channels {
                            let sample = buffer.data[ch][i];
                            let val = (sample * scale_factor).round() as i32;
                            local_buffer.push(val);
                        }
                    }

                    local_buffer
                })
                .collect()
        } else {
            // For smaller files, use a straightforward sequential approach
            // which avoids overhead of parallelism for small datasets
            let mut samples = Vec::with_capacity(num_samples * channels);
            for i in 0..num_samples {
                for ch in 0..channels {
                    let sample = buffer.data[ch][i];
                    let val = (sample * scale_factor).round() as i32;
                    samples.push(val);
                }
            }
            samples
        };

        // Configure the encoder with optimized settings
        let mut config = flacenc::config::Encoder::default();

        // Set larger block size for better throughput and compression
        config.block_size = 8192;

        // Create a verified config
        let config = config
            .into_verified()
            .map_err(|e| anyhow!("Invalid FLAC encoder configuration: {:?}", e))?;

        // Create a source from the interleaved samples
        let source = flacenc::source::MemSource::from_samples(
            &interleaved_samples,
            channels,
            bits_per_sample as usize,
            sample_rate,
        );

        // Use a fixed block size for consistent performance
        let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
            .map_err(|e| anyhow!("FLAC encoding error: {:?}", e))?;

        // Estimate final buffer size (typically FLAC is ~50-60% of raw PCM)
        let estimated_size = (num_samples * channels * (bits_per_sample as usize / 8) / 2) + 8192;

        // Create a byte sink with sufficient capacity
        let mut sink = flacenc::bitsink::ByteSink::new();
        sink.reserve(estimated_size);

        // Write the encoded stream
        flac_stream.write(&mut sink)?;

        // Return the encoded FLAC data
        Ok(sink.as_slice().to_vec())
    }

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata> {
        let mut metadata = Metadata::new();

        // First, try to parse FLAC metadata blocks using metaflac
        let temp_file = std::env::temp_dir().join("temp_flac_metadata");
        std::fs::write(&temp_file, input)?;

        if let Ok(tag) = Tag::read_from_path(&temp_file) {
            // Parse Vorbis comments
            if let Some(comments) = tag.vorbis_comments() {
                for (key, values) in &comments.comments {
                    if !values.is_empty() {
                        let standard_key = self.normalize_vorbis_key(key);
                        metadata.set_field(&standard_key, &values[0])?;
                    }
                }
            }

            // Parse Application blocks for iXML
            for block in tag.blocks() {
                if let Block::Application(app_block) = block {
                    if &app_block.id == b"iXML" {
                        let ixml_str = String::from_utf8_lossy(&app_block.data);
                        metadata.parse_ixml(&ixml_str)?;
                    }
                }
            }
        }

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_file);

        // Also parse any embedded chunks manually from the FLAC stream
        let mut cursor = Cursor::new(input);

        // Skip FLAC marker "fLaC"
        if input.len() >= 4 && &input[0..4] == b"fLaC" {
            cursor.set_position(4);

            // Parse metadata blocks
            loop {
                if cursor.position() >= input.len() as u64 {
                    break;
                }

                let block_header = match cursor.read_u8() {
                    Ok(header) => header,
                    Err(_) => break,
                };

                let is_last = (block_header & 0x80) != 0;
                let block_type = block_header & 0x7F;

                let block_size = match cursor.read_u24::<byteorder::BigEndian>() {
                    Ok(size) => size as usize,
                    Err(_) => break,
                };

                let block_start = cursor.position() as usize;
                if block_start + block_size > input.len() {
                    break;
                }

                let block_data = &input[block_start..block_start + block_size];

                match block_type {
                    // APPLICATION block - might contain iXML or other metadata
                    2 => {
                        if block_data.len() >= 4 {
                            let app_id = &block_data[0..4];
                            let app_data = &block_data[4..];

                            if app_id == b"iXML" {
                                if let Ok(xml_str) = std::str::from_utf8(app_data) {
                                    metadata.parse_ixml(xml_str)?;
                                }
                            } else if app_id == b"SMED" {
                                // Soundminer metadata
                                metadata.set_field("Soundminer", "present")?;
                            }
                        }
                    }
                    // VORBIS_COMMENT block is already handled by metaflac above
                    // PADDING, STREAMINFO, etc. - skip
                    _ => {}
                }

                cursor.set_position(block_start as u64 + block_size as u64);

                if is_last {
                    break;
                }
            }
        }

        Ok(metadata)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()> {
        // Use metaflac to safely write metadata blocks
        let mut dest_tag = Tag::read_from_path(file_path).unwrap_or_else(|_| Tag::new());

        // Clear existing metadata blocks that we're about to replace
        dest_tag.remove_blocks(metaflac::BlockType::VorbisComment);
        dest_tag.remove_blocks(metaflac::BlockType::Picture);
        dest_tag.remove_blocks(metaflac::BlockType::Application);

        // Create a new VorbisComment block from the hashmap
        let mut vorbis_comment = metaflac::block::VorbisComment::new();
        vorbis_comment.vendor_string = "FFCodex".to_string();

        // Add all fields from the hashmap to VorbisComment
        for (key, value) in metadata.get_all_fields().iter() {
            // Convert to standard Vorbis comment field names
            let vorbis_key = self.map_to_vorbis_key(key);
            vorbis_comment
                .comments
                .entry(vorbis_key)
                .or_default()
                .push(value.clone());
        }

        // Add the VorbisComment block
        dest_tag.push_block(Block::VorbisComment(vorbis_comment));

        // Add image chunks as Picture blocks
        for image in metadata.get_images() {
            let picture_block = metaflac::block::Picture {
                picture_type: metaflac::block::PictureType::Other,
                mime_type: image.mime_type().to_string(),
                description: image.description().to_string(),
                width: 0,
                height: 0,
                depth: 0,
                num_colors: 0,
                data: image.data().to_vec(),
            };
            dest_tag.push_block(Block::Picture(picture_block));
        }

        // Add iXML as Application block (BWF chunk)
        let ixml_content = self.create_ixml(metadata)?;
        let ixml_block = metaflac::block::Application {
            id: b"iXML".to_vec(),
            data: ixml_content.as_bytes().to_vec(),
        };
        dest_tag.push_block(Block::Application(ixml_block));

        // Write the metadata back to the file
        dest_tag
            .write_to_path(file_path)
            .map_err(|e| anyhow!("Failed to write FLAC metadata: {}", e))?;

        Ok(())
    }
}

impl FlacCodec {
    fn normalize_vorbis_key(&self, key: &str) -> String {
        match key.to_uppercase().as_str() {
            "TITLE" => "Title".to_string(),
            "ARTIST" => "Artist".to_string(),
            "ALBUM" => "Album".to_string(),
            "DATE" => "Year".to_string(),
            "GENRE" => "Genre".to_string(),
            "TRACKNUMBER" => "Track".to_string(),
            "ALBUMARTIST" => "AlbumArtist".to_string(),
            "COMPOSER" => "Composer".to_string(),
            "CONDUCTOR" => "Conductor".to_string(),
            "COMMENT" => "Comment".to_string(),
            "DESCRIPTION" => "Description".to_string(),
            "DISCNUMBER" => "DiscNumber".to_string(),
            "ORGANIZATION" => "Publisher".to_string(),
            "CONTACT" => "Contact".to_string(),
            "COPYRIGHT" => "Copyright".to_string(),
            "ISRC" => "ISRC".to_string(),
            "ENCODER" => "EncodingSettings".to_string(),
            "LANGUAGE" => "Language".to_string(),
            "PERFORMER" => "Performer".to_string(),
            "VERSION" => "Version".to_string(),
            "LOCATION" => "Location".to_string(),
            _ => {
                // Preserve WAV-specific prefixed fields for cross-format compatibility
                if key.starts_with("USER_")
                    || key.starts_with("BEXT_")
                    || key.starts_with("ASWG_")
                    || key.starts_with("STEINBERG_")
                {
                    key.to_string()
                } else {
                    key.to_string()
                }
            }
        }
    }
    fn map_to_vorbis_key(&self, key: &str) -> String {
        // Map common metadata keys to standard Vorbis comment field names
        match key {
            "Title" => "TITLE".to_string(),
            "Artist" => "ARTIST".to_string(),
            "Album" => "ALBUM".to_string(),
            "Year" => "DATE".to_string(),
            "Genre" => "GENRE".to_string(),
            "Track" => "TRACKNUMBER".to_string(),
            "AlbumArtist" => "ALBUMARTIST".to_string(),
            "Composer" => "COMPOSER".to_string(),
            "Conductor" => "CONDUCTOR".to_string(),
            "Comment" => "COMMENT".to_string(),
            "Description" => "DESCRIPTION".to_string(),
            "DiscNumber" => "DISCNUMBER".to_string(),
            "Publisher" => "ORGANIZATION".to_string(),
            "Contact" => "CONTACT".to_string(),
            "Copyright" => "COPYRIGHT".to_string(),
            "ISRC" => "ISRC".to_string(),
            "EncodingSettings" => "ENCODER".to_string(),
            "Language" => "LANGUAGE".to_string(),
            "Performer" => "PERFORMER".to_string(),
            "Version" => "VERSION".to_string(),
            "Location" => "LOCATION".to_string(),
            // For any other keys, convert to uppercase (Vorbis convention)
            // But preserve WAV-specific prefixed fields as-is for cross-format compatibility
            _ => {
                if key.starts_with("USER_")
                    || key.starts_with("BEXT_")
                    || key.starts_with("ASWG_")
                    || key.starts_with("STEINBERG_")
                {
                    key.to_string()
                } else {
                    key.to_uppercase()
                }
            }
        }
    }
}

// Helper function to get bits per sample from SampleFormat
fn get_bits_per_sample(format: SampleFormat) -> u16 {
    match format {
        SampleFormat::U8 => 8,
        SampleFormat::I16 => 16,
        SampleFormat::I24 => 24,
        SampleFormat::I32 | SampleFormat::F32 => 32,
    }
}

// Helper function to select sample format based on bit depth
fn select_sample_format(bits_per_sample: u16) -> SampleFormat {
    match bits_per_sample {
        8 => SampleFormat::U8,
        16 => SampleFormat::I16,
        24 => SampleFormat::I24,
        32 => SampleFormat::I32,
        // Map non-standard bit depths to closest format
        1..=12 => SampleFormat::I16,
        13..=20 => SampleFormat::I16,
        21..=28 => SampleFormat::I24,
        _ => SampleFormat::I32,
    }
}
