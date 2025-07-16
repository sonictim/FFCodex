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
const PICTURE_BLOCK_TYPE: u8 = 6;
const LAST_METADATA_BLOCK_FLAG: u8 = 0x80;

// Sample normalization constants
const I16_MAX_F: f32 = 32767.0;
const I16_DIVISOR: f32 = 32768.0;
const I24_MAX_F: f32 = 8388607.0;
const I24_DIVISOR: f32 = 8388608.0;
const I32_MAX_F: f32 = 2147483647.0;
const I32_DIVISOR: f32 = 2147483648.0;

pub struct FlacCodec;

// impl FlacCodec {
//     fn extract_metadata_chunks_from_file(&self, file_path: &str) -> R<Vec<MetadataChunk>> {
//         let data = std::fs::read(file_path)?;
//         self.extract_metadata_chunks(&data)
//     }
// }

impl Codec for FlacCodec {
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

        // OPTIMIZATION: Direct sample iteration with intelligent buffer sizing
        if samples_per_channel < 10_000 || num_samples == 0 {
            // For small files or unknown size, process samples directly
            let mut sample_buffer = Vec::with_capacity(channel_count);

            // Process samples manually grouped by channel count
            sample_buffer.clear();
            for sample_result in reader.samples() {
                match sample_result {
                    Ok(sample) => {
                        sample_buffer.push((sample as f32) / divisor);

                        // When we have collected one sample for each channel
                        if sample_buffer.len() == channel_count {
                            // Distribute to channels
                            for (ch, &sample) in
                                sample_buffer.iter().enumerate().take(channel_count)
                            {
                                audio_data[ch].push(sample);
                            }
                            sample_buffer.clear();
                        }
                    }
                    Err(e) => return Err(anyhow!("Error reading FLAC samples: {}", e)),
                }
            }
        } else {
            // For larger files with known size, use parallel processing
            // OPTIMIZATION: Process directly from reader and avoid extra allocations
            if samples_per_channel > 100_000 {
                // Collect blocks of interleaved samples for parallel processing
                let block_size = (samples_per_channel / rayon::current_num_threads()).max(1024);
                let blocks_needed = samples_per_channel.div_ceil(block_size);

                let mut sample_blocks: Vec<Vec<i32>> = Vec::with_capacity(blocks_needed);
                let mut current_block = Vec::with_capacity(block_size * channel_count);

                for sample_result in reader.samples() {
                    let sample = sample_result?;
                    current_block.push(sample);

                    // When we've filled a block, store it and start a new one
                    if current_block.len() == block_size * channel_count {
                        sample_blocks.push(std::mem::take(&mut current_block));
                        current_block = Vec::with_capacity(block_size * channel_count);
                    }
                }

                // Don't forget any remaining samples
                if !current_block.is_empty() {
                    sample_blocks.push(current_block);
                }

                // Process blocks in parallel
                let results: Vec<Vec<Vec<f32>>> = sample_blocks
                    .par_iter()
                    .map(|block| {
                        let mut local_channels =
                            vec![Vec::with_capacity(block.len() / channel_count); channel_count];

                        for chunk in block.chunks_exact(channel_count) {
                            for (ch, &sample) in chunk.iter().enumerate() {
                                local_channels[ch].push(sample as f32 / divisor);
                            }
                        }

                        local_channels
                    })
                    .collect();

                // Combine results
                for (ch, channel_vec) in audio_data.iter_mut().enumerate() {
                    for result in &results {
                        channel_vec.extend_from_slice(&result[ch]);
                    }
                }
            } else {
                // For medium files, use a more efficient sequential approach
                // OPTIMIZATION: Batch processing for better cache locality
                const BATCH_SIZE: usize = 4096; // Adjust based on your system's cache

                // Process in batches for better cache performance
                while let Ok(batch) = reader
                    .samples()
                    .take(BATCH_SIZE * channel_count)
                    .collect::<Result<Vec<i32>, _>>()
                {
                    if batch.is_empty() {
                        break;
                    }

                    // Process this batch
                    for chunk in batch.chunks_exact(channel_count) {
                        for (ch, &sample) in chunk.iter().enumerate() {
                            audio_data[ch].push(sample as f32 / divisor);
                        }
                    }
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

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata> {
        let tag = Tag::read_from_path(file_path)?;
        let file_data = std::fs::read(file_path)?;
        let chunks = self.extract_metadata_chunks(&file_data)?;

        Ok(Metadata::Flac(tag, chunks))
    }

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata> {
        let mut metadata = Metadata::new();
        
        // First, try to parse FLAC metadata blocks using metaflac
        let temp_file = std::env::temp_dir().join("temp_flac_metadata");
        std::fs::write(&temp_file, input)?;
        
        if let Ok(tag) = Tag::read_from_path(&temp_file) {
            // Parse Vorbis comments
            if let Some(comments) = tag.vorbis_comments() {
                for (key, values) in comments.iter() {
                    if !values.is_empty() {
                        let standard_key = self.normalize_vorbis_key(key);
                        metadata.set_field(&standard_key, &values[0])?;
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

    fn normalize_vorbis_key(&self, key: &str) -> String {
        match key.to_uppercase().as_str() {
            "TITLE" => "Title",
            "ARTIST" => "Artist",
            "ALBUM" => "Album",
            "DATE" => "Year",
            "GENRE" => "Genre",
            "TRACKNUMBER" => "Track",
            "ALBUMARTIST" => "AlbumArtist",
            "COMPOSER" => "Composer",
            "CONDUCTOR" => "Conductor",
            "COMMENT" => "Comment",
            "DESCRIPTION" => "Description",
            "DISCNUMBER" => "DiscNumber",
            "ORGANIZATION" => "Publisher",
            "CONTACT" => "Contact",
            "COPYRIGHT" => "Copyright",
            "ISRC" => "ISRC",
            "ENCODER" => "EncodingSettings",
            "LANGUAGE" => "Language",
            "PERFORMER" => "Performer",
            "VERSION" => "Version",
            "LOCATION" => "Location",
            _ => key,
        }.to_string()
    }

    // Helper methods for parsing specific chunk types have been moved to centralized functions in codecs.rs

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>> {
        let mut cursor = Cursor::new(input);

        // Skip FLAC marker
        cursor.seek(SeekFrom::Start(4))?;

        let mut chunks = Vec::new();
        let mut last_metadata_block = false;
        let mut found_relevant_metadata = false;

        // Parse metadata blocks - only collect the first relevant metadata block (iXML or Vorbis)
        while !last_metadata_block && !found_relevant_metadata {
            let header = cursor.read_u8()?;
            last_metadata_block = (header & LAST_METADATA_BLOCK_FLAG) != 0;
            let block_type = header & 0x7F;
            let block_size = cursor.read_u24::<BigEndian>()? as usize;

            let mut data = vec![0u8; block_size];
            cursor.read_exact(&mut data)?;

            match block_type {
                VORBIS_COMMENT_BLOCK_TYPE => {
                    // Parse Vorbis comment data according to the format specification
                    if data.len() >= 4 {
                        let mut data_cursor = Cursor::new(&data);

                        // Read vendor length and vendor string
                        let vendor_length = data_cursor.read_u32::<LittleEndian>()?;
                        let mut vendor = vec![0u8; vendor_length as usize];
                        data_cursor.read_exact(&mut vendor)?;

                        // Read user comment list
                        let comment_list_length = data_cursor.read_u32::<LittleEndian>()?;

                        // Extract key-value pairs
                        let mut text_tags = Vec::new();
                        let mut comments = String::new();
                        comments
                            .push_str(&format!("VENDOR={}\n", String::from_utf8_lossy(&vendor)));

                        for _ in 0..comment_list_length {
                            if data_cursor.position() >= data.len() as u64 {
                                break;
                            }

                            let comment_length = match data_cursor.read_u32::<LittleEndian>() {
                                Ok(len) => len,
                                Err(_) => break,
                            };

                            if comment_length > 0
                                && data_cursor.position() + comment_length as u64
                                    <= data.len() as u64
                            {
                                let mut comment_data = vec![0u8; comment_length as usize];
                                if data_cursor.read_exact(&mut comment_data).is_ok() {
                                    if let Ok(comment) = String::from_utf8(comment_data) {
                                        comments.push_str(&format!("{}\n", comment));

                                        // Also create TextTag entries for better cross-format compatibility
                                        if let Some(idx) = comment.find('=') {
                                            let key = comment[0..idx].trim().to_string();
                                            let value = comment[idx + 1..].trim().to_string();
                                            text_tags.push(MetadataChunk::TextTag { key, value });
                                        }
                                    }
                                }
                            }
                        }

                        // Add only the TextTag format - Vorbis comments are not iXML
                        chunks.extend(text_tags);
                        found_relevant_metadata = true; // Stop after finding first relevant metadata
                    }
                }
                2 => {
                    // APPLICATION block - check for iXML specifically
                    if data.len() >= 4 {
                        // Read the application ID (first 4 bytes)
                        let app_id = &data[0..4];
                        let app_data = &data[4..];

                        match app_id {
                            b"iXML" => {
                                // Only process if we haven't found relevant metadata yet
                                if !found_relevant_metadata {
                                    // Parse iXML data - this is what we really want
                                    if let Ok(xml_string) = String::from_utf8(app_data.to_vec()) {
                                        chunks.push(MetadataChunk::IXml(xml_string));
                                        found_relevant_metadata = true; // Mark that we found our metadata
                                    }
                                }
                            }
                            b"smgz" | b"SMED" | b"SMRD" | b"SMPL" => {
                                // Capture SMED (Soundminer) blocks
                                chunks.push(MetadataChunk::Soundminer(app_data.to_vec()));
                            }
                            _ => {
                                // Skip all other application blocks - we only care about iXML
                            }
                        }
                    }
                }
                // Skip other metadata types - we only care about Vorbis comments and iXML
                _ => {
                    // Skip this block
                }
            }
        }

        Ok(chunks)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Option<Metadata>) -> R<()> {
        let Some(metadata) = metadata else {
            return Err(anyhow!("Cannot embed None Metadata"));
        };

        let (tag, chunks) = match metadata {
            Metadata::Flac(tag, chunks) => (tag, chunks),
            _ => return Err(anyhow!("Unsupported metadata format")),
        };

        dprintln!(
            "embed_metadata_to_file: Starting with {} chunks",
            chunks.len()
        );

        // Use metaflac to safely write metadata blocks
        let mut dest_tag = Tag::read_from_path(file_path).unwrap_or_else(|_| Tag::new());

        // Clear existing metadata blocks that we're about to replace
        dest_tag.remove_blocks(metaflac::BlockType::VorbisComment);
        dest_tag.remove_blocks(metaflac::BlockType::Picture);
        dest_tag.remove_blocks(metaflac::BlockType::Application);

        // Collect the IDs of chunks we're going to replace
        let mut replacing_app_ids = std::collections::HashSet::new();
        for chunk in chunks {
            match chunk {
                MetadataChunk::IXml(_) => {
                    replacing_app_ids.insert(b"iXML".to_vec());
                }
                MetadataChunk::Unknown { id, .. } if id.starts_with("FLAC_APP_") => {
                    let app_id_str = id.trim_start_matches("FLAC_APP_");
                    let app_id = if app_id_str.len() >= 4 {
                        app_id_str.as_bytes()[0..4].to_vec()
                    } else {
                        let mut id_bytes = app_id_str.as_bytes().to_vec();
                        id_bytes.resize(4, 0);
                        id_bytes
                    };
                    replacing_app_ids.insert(app_id);
                }
                _ => {}
            }
        }

        // Add only Application blocks from the source tag that we're not replacing
        // We skip VorbisComment and Picture blocks since we'll recreate them from chunks
        for block in tag.blocks() {
            match block {
                Block::Application(app_block) => {
                    if !replacing_app_ids.contains(&app_block.id) {
                        dest_tag.push_block(block.clone());
                    }
                }
                _ => {
                    // Skip other blocks - we'll recreate VorbisComment and Picture from chunks
                }
            }
        }

        // Consolidate all text-based metadata into a single VorbisComment block
        let mut consolidated_vorbis_comment = metaflac::block::VorbisComment::new();
        consolidated_vorbis_comment.vendor_string = "FFCodex".to_string();
        let mut has_vorbis_data = false;

        // Process chunks and convert them to metaflac blocks
        for chunk in chunks {
            match chunk {
                MetadataChunk::IXml(xml_string) => {
                    // Check if this is proper iXML data (contains XML structure)
                    if xml_string.contains("<?xml")
                        || xml_string.contains("<BWFXML>")
                        || xml_string.contains("<BWF_IXML")
                    {
                        // This is iXML data - create APPLICATION block
                        let app_block = metaflac::block::Application {
                            id: b"iXML".to_vec(),
                            data: xml_string.as_bytes().to_vec(),
                        };
                        dest_tag.push_block(Block::Application(app_block));
                    } else {
                        // This is Vorbis comment data (legacy format)
                        has_vorbis_data = true;
                        for line in xml_string.lines() {
                            if line.starts_with("VENDOR=") {
                                consolidated_vorbis_comment.vendor_string =
                                    line.trim_start_matches("VENDOR=").to_string();
                            } else if !line.is_empty() && line.contains('=') {
                                if let Some((key, value)) = line.split_once('=') {
                                    consolidated_vorbis_comment
                                        .comments
                                        .entry(key.to_string())
                                        .or_default()
                                        .push(value.to_string());
                                }
                            }
                        }
                    }
                }
                MetadataChunk::TextTag { key, value } => {
                    // Add to consolidated Vorbis comment
                    has_vorbis_data = true;
                    consolidated_vorbis_comment
                        .comments
                        .entry(key.clone())
                        .or_default()
                        .push(value.clone());
                }
                MetadataChunk::Picture {
                    mime_type,
                    description,
                    data,
                } => {
                    let picture_block = metaflac::block::Picture {
                        picture_type: metaflac::block::PictureType::Other,
                        mime_type: mime_type.clone(),
                        description: description.clone(),
                        width: 0,
                        height: 0,
                        depth: 0,
                        num_colors: 0,
                        data: data.clone(),
                    };
                    dest_tag.push_block(Block::Picture(picture_block));
                }
                MetadataChunk::Unknown { id, data } => {
                    if id.starts_with("FLAC_APP_") {
                        // Extract application ID
                        let app_id_str = id.trim_start_matches("FLAC_APP_");
                        let app_id = if app_id_str.len() >= 4 {
                            app_id_str.as_bytes()[0..4].to_vec()
                        } else {
                            let mut id_bytes = app_id_str.as_bytes().to_vec();
                            id_bytes.resize(4, 0);
                            id_bytes
                        };

                        let app_block = metaflac::block::Application {
                            id: app_id,
                            data: data.clone(),
                        };
                        dest_tag.push_block(Block::Application(app_block));
                    }
                    // Skip other unknown blocks for now
                }
                MetadataChunk::Soundminer(data) => {
                    // Preserve Soundminer chunks as APPLICATION blocks
                    // Use "smgz" as the application ID for Soundminer data
                    let app_block = metaflac::block::Application {
                        id: b"smgz".to_vec(),
                        data: data.clone(),
                    };
                    dest_tag.push_block(Block::Application(app_block));
                }
                _ => {
                    // Skip other chunk types for now
                }
            }
        }

        // Add the consolidated VorbisComment block only if we have Vorbis data
        if has_vorbis_data {
            dest_tag.push_block(Block::VorbisComment(consolidated_vorbis_comment));
        }

        // Write the metadata back to the file
        dest_tag
            .write_to_path(file_path)
            .map_err(|e| anyhow!("Failed to write FLAC metadata: {}", e))?;

        Ok(())
    }

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>> {
        // Skip processing if there are no chunks to embed
        if chunks.is_empty() {
            return Ok(input.to_vec());
        }

        let mut cursor = Cursor::new(input);
        let mut output = Cursor::new(Vec::new());

        // Copy FLAC marker - never modify this
        let mut marker = [0u8; 4];
        cursor.read_exact(&mut marker)?;
        output.write_all(&marker)?;

        // Read the STREAMINFO block header
        let header = cursor.read_u8()?;
        let block_type = header & 0x7F;
        let block_size = cursor.read_u24::<BigEndian>()? as usize;

        if block_type != STREAMINFO_BLOCK_TYPE {
            return Err(anyhow!("First metadata block is not STREAMINFO"));
        }

        // Copy STREAMINFO data - don't modify this either
        let mut streaminfo_data = vec![0u8; block_size];
        cursor.read_exact(&mut streaminfo_data)?;

        // Write the STREAMINFO header exactly as it was, but clear the LAST flag
        // since we're adding metadata
        output.write_u8(STREAMINFO_BLOCK_TYPE)?; // Always clear the last block flag
        output.write_u24::<BigEndian>(block_size as u32)?;
        output.write_all(&streaminfo_data)?;

        // Collect and organize new metadata chunks
        let mut vorbis_chunks = Vec::new();
        let mut picture_chunks = Vec::new();
        let mut other_chunks = Vec::new();
        let mut text_tags = Vec::new();

        for chunk in chunks {
            match chunk {
                MetadataChunk::IXml(_) => vorbis_chunks.push(chunk.clone()),
                MetadataChunk::Picture { .. } => picture_chunks.push(chunk.clone()),
                MetadataChunk::TextTag { .. } => text_tags.push(chunk.clone()),
                _ => other_chunks.push(chunk.clone()),
            }
        }

        // Group TextTag entries into a Vorbis comment if not already present
        if !text_tags.is_empty() && vorbis_chunks.is_empty() {
            let mut xml = String::from_utf8_lossy(b"VENDOR=FFCodex\n").to_string();
            for tag in &text_tags {
                if let MetadataChunk::TextTag { key, value } = tag {
                    xml.push_str(&format!("{}={}\n", key, value));
                }
            }
            vorbis_chunks.push(MetadataChunk::IXml(xml));
        }

        // Collect all metadata blocks
        let all_chunks: Vec<&MetadataChunk> = vorbis_chunks
            .iter()
            .chain(picture_chunks.iter())
            .chain(other_chunks.iter())
            .collect();

        // Now write the new metadata blocks
        for (i, chunk) in all_chunks.iter().enumerate() {
            let is_last = i == all_chunks.len() - 1;

            let (block_type, data) = match chunk {
                MetadataChunk::IXml(xml_string) => {
                    // Convert IXml string to Vorbis comment format
                    let mut vorbis_data = Cursor::new(Vec::new());
                    let mut vendor = b"FFCodex".to_vec();
                    let mut comments = Vec::new();

                    for line in xml_string.lines() {
                        if line.starts_with("VENDOR=") {
                            vendor = line.trim_start_matches("VENDOR=").as_bytes().to_vec();
                        } else if !line.is_empty() {
                            comments.push(line.as_bytes().to_vec());
                        }
                    }

                    vorbis_data.write_u32::<LittleEndian>(vendor.len() as u32)?;
                    vorbis_data.write_all(&vendor)?;
                    vorbis_data.write_u32::<LittleEndian>(comments.len() as u32)?;

                    for comment in comments {
                        vorbis_data.write_u32::<LittleEndian>(comment.len() as u32)?;
                        vorbis_data.write_all(&comment)?;
                    }

                    (VORBIS_COMMENT_BLOCK_TYPE, vorbis_data.into_inner())
                }
                MetadataChunk::Picture {
                    mime_type,
                    description,
                    data,
                } => {
                    let mut pic_data = Cursor::new(Vec::new());
                    pic_data.write_u32::<BigEndian>(0)?; // Picture type
                    pic_data.write_u32::<BigEndian>(mime_type.len() as u32)?;
                    pic_data.write_all(mime_type.as_bytes())?;
                    pic_data.write_u32::<BigEndian>(description.len() as u32)?;
                    pic_data.write_all(description.as_bytes())?;

                    // Width, height, color depth, colors used
                    pic_data.write_u32::<BigEndian>(0)?;
                    pic_data.write_u32::<BigEndian>(0)?;
                    pic_data.write_u32::<BigEndian>(0)?;
                    pic_data.write_u32::<BigEndian>(0)?;

                    pic_data.write_u32::<BigEndian>(data.len() as u32)?;
                    pic_data.write_all(data)?;

                    (PICTURE_BLOCK_TYPE, pic_data.into_inner())
                }
                MetadataChunk::Unknown { id, data } if id == "FLAC_PICTURE" => {
                    (PICTURE_BLOCK_TYPE, data.clone())
                }
                MetadataChunk::TextTag { .. } => {
                    continue; // Skip individual text tags
                }
                MetadataChunk::Soundminer(data) => {
                    // Preserve Soundminer chunks as APPLICATION blocks
                    // Use "smgz" as the application ID for Soundminer data
                    let mut app_data = Vec::with_capacity(4 + data.len());
                    app_data.extend_from_slice(b"smgz");
                    app_data.extend_from_slice(data);
                    (2, app_data) // Application block type
                }
                MetadataChunk::Unknown { id, data } => {
                    let block_type = if id.starts_with("FLAC_") {
                        id.trim_start_matches("FLAC_").parse::<u8>().unwrap_or(0x7F) & 0x7F
                    } else {
                        0x7F
                    };
                    (block_type, data.clone())
                }
                _ => (0x7F, chunk.data().to_vec()),
            };

            // Write the header for our new metadata block
            output.write_u8(if is_last {
                block_type | LAST_METADATA_BLOCK_FLAG
            } else {
                block_type
            })?;

            // Write the block size and data
            output.write_u24::<BigEndian>(data.len() as u32)?;
            output.write_all(&data)?;
        }

        // Now we need to read (and skip) all original metadata blocks after STREAMINFO
        // The first block we already read (STREAMINFO)
        let mut last_metadata_block = (header & LAST_METADATA_BLOCK_FLAG) != 0;

        // If the original STREAMINFO was the last metadata block, we're done with metadata
        // Otherwise, copy all remaining original metadata blocks
        if !last_metadata_block {
            let mut original_metadata_blocks = Vec::new();

            // First, read all metadata blocks to memory
            while !last_metadata_block {
                // Read block header
                let header = cursor.read_u8()?;
                last_metadata_block = (header & LAST_METADATA_BLOCK_FLAG) != 0;
                let block_type = header & 0x7F;
                let block_size = cursor.read_u24::<BigEndian>()? as usize;

                // Read block data
                let mut block_data = vec![0u8; block_size];
                cursor.read_exact(&mut block_data)?;

                // Store this metadata block (header and all) for later
                original_metadata_blocks.push((block_type, block_data));
            }

            // Now write all original metadata blocks (if any)
            // but set the LAST flag only on the very last one
            if !original_metadata_blocks.is_empty() {
                // If we added our own blocks, clear the LAST flag on our last one
                if !all_chunks.is_empty() {
                    // Go back and clear the LAST flag on our last written metadata block
                    let current_position = output.position();
                    let last_header_pos =
                        current_position - all_chunks.last().unwrap().data().len() as u64 - 4;
                    output.seek(SeekFrom::Start(last_header_pos))?;

                    let last_block_type = match all_chunks.last().unwrap() {
                        MetadataChunk::IXml(_) => VORBIS_COMMENT_BLOCK_TYPE,
                        MetadataChunk::Picture { .. } => PICTURE_BLOCK_TYPE,
                        MetadataChunk::Unknown { id, .. } if id == "FLAC_PICTURE" => {
                            PICTURE_BLOCK_TYPE
                        }
                        MetadataChunk::Unknown { id, .. } if id.starts_with("FLAC_") => {
                            id.trim_start_matches("FLAC_").parse::<u8>().unwrap_or(0x7F) & 0x7F
                        }
                        _ => 0x7F,
                    };

                    // Write without LAST flag
                    output.write_u8(last_block_type)?;

                    // Restore the position
                    output.seek(SeekFrom::Start(current_position))?;
                }

                // Write all but the last original metadata block
                for (i, (block_type, block_data)) in original_metadata_blocks.iter().enumerate() {
                    let is_last = i == original_metadata_blocks.len() - 1;

                    // Write the header
                    output.write_u8(if is_last {
                        block_type | LAST_METADATA_BLOCK_FLAG
                    } else {
                        *block_type
                    })?;

                    // Write the block size and data
                    output.write_u24::<BigEndian>(block_data.len() as u32)?;
                    output.write_all(block_data)?;
                }
            }
        }

        // Finally, copy all audio frames (everything after metadata)
        let _position = cursor.position();
        let mut audio_data = Vec::new();
        cursor.read_to_end(&mut audio_data)?;
        output.write_all(&audio_data)?;

        Ok(output.into_inner())
    }

    // fn embed_file_metadata_chunks(&self, file_path: &str, chunks: &[MetadataChunk]) -> R<()> {
    //     if !file_path.to_lowercase().ends_with(".flac") {
    //         // Use your existing implementation for non-FLAC files
    //         let file = std::fs::File::open(file_path)?;
    //         let mapped_file = unsafe { MmapOptions::new().map(&file)? };
    //         let new_data = self.embed_metadata_chunks(&mapped_file, chunks)?;
    //         std::fs::write(file_path, new_data)?;
    //         return Ok(());
    //     }

    //     // For FLAC files, use metaflac
    //     use metaflac::Tag;
    //     use metaflac::block::{Block, VorbisComment};

    //     // Open the FLAC file with metaflac
    //     let mut tag = match Tag::read_from_path(file_path) {
    //         Ok(tag) => tag,
    //         Err(_) => Tag::new(),
    //     };

    //     // Remove existing vorbis comments if we're adding new ones
    //     if chunks
    //         .iter()
    //         .any(|c| matches!(c, MetadataChunk::IXml(_) | MetadataChunk::TextTag { .. }))
    //     {
    //         tag.remove_blocks(metaflac::BlockType::VorbisComment);
    //     }

    //     // Add metadata blocks
    //     for chunk in chunks {
    //         match chunk {
    //             MetadataChunk::IXml(xml_string) => {
    //                 // Parse XML string into vorbis comments
    //                 let mut comment = VorbisComment::new();
    //                 for line in xml_string.lines() {
    //                     if let Some((key, value)) = line.split_once('=') {
    //                         if key != "VENDOR" {
    //                             comment.comments.push(format!("{}={}", key, value));
    //                         } else {
    //                             comment.vendor_string = value.to_string();
    //                         }
    //                     }
    //                 }
    //                 tag.add_block(Block::VorbisComment(comment));
    //             }
    //             MetadataChunk::TextTag { key, value } => {
    //                 // Find or create VorbisComment block
    //                 let vorbis_comment = tag.get_or_insert_block::<VorbisComment>();
    //                 // Add comment
    //                 vorbis_comment.comments.push(format!("{}={}", key, value));
    //             }
    //             // Handle other metadata types as needed
    //             _ => {}
    //         }
    //     }

    //     // Write the modified tag back to the file
    //     tag.write_to_path(file_path)
    //         .map_err(|e| anyhow!("Failed to write FLAC metadata: {}", e))
    // }
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
