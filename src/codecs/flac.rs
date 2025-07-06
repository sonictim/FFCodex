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
        let input = std::fs::read(file_path)?;
        let chunks = self.extract_metadata_chunks(&input)?;

        Ok(Metadata::Flac(tag, chunks))
    }

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>> {
        let mut cursor = Cursor::new(input);

        // Skip FLAC marker
        cursor.seek(SeekFrom::Start(4))?;

        let mut chunks = Vec::new();
        let mut last_metadata_block = false;

        // Parse metadata blocks
        while !last_metadata_block {
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
                        if vendor_length as usize > data.len() - 4 {
                            // Invalid vendor length, skip this block
                            chunks.push(MetadataChunk::Unknown {
                                id: "FLAC_VORBIS_COMMENT".to_string(),
                                data: data.to_vec(),
                            });
                            continue;
                        }

                        let mut vendor = vec![0u8; vendor_length as usize];
                        data_cursor.read_exact(&mut vendor)?;

                        // Read user comment list
                        if data_cursor.position() + 4 > data.len() as u64 {
                            // Not enough data for comment count
                            chunks.push(MetadataChunk::Unknown {
                                id: "FLAC_VORBIS_COMMENT".to_string(),
                                data: data.to_vec(),
                            });
                            continue;
                        }

                        let comment_list_length = data_cursor.read_u32::<LittleEndian>()?;

                        // Extract key-value pairs and check for IXML content
                        let mut text_tags = Vec::new();
                        let mut ixml_content = None;
                        let mut vorbis_comments = Vec::new();

                        // Store vendor information
                        let vendor_string = String::from_utf8_lossy(&vendor).to_string();
                        vorbis_comments.push(format!("VENDOR={}", vendor_string));

                        for _ in 0..comment_list_length {
                            if data_cursor.position() + 4 > data.len() as u64 {
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
                                    // First check if this is raw IXML data (starts with "iXML" or "<BWFXML>")
                                    if comment_data.len() > 4
                                        && (comment_data.starts_with(b"iXML")
                                            || comment_data.starts_with(b"<BWFXML>")
                                            || String::from_utf8_lossy(&comment_data)
                                                .contains("<BWFXML>"))
                                    {
                                        // This is raw IXML content
                                        let ixml_string =
                                            String::from_utf8_lossy(&comment_data).to_string();
                                        vorbis_comments
                                            .push(format!("IXML_RAW_DATA={}", ixml_string));
                                        ixml_content = Some(ixml_string);
                                        continue;
                                    }

                                    // Try to parse as UTF-8 string
                                    if let Ok(comment) = String::from_utf8(comment_data.clone()) {
                                        vorbis_comments.push(comment.clone());

                                        // Check if this is a key=value pair
                                        if let Some(idx) = comment.find('=') {
                                            let key = comment[0..idx].trim();
                                            let value = comment[idx + 1..].trim();

                                            // Check for IXML metadata specifically
                                            if key.eq_ignore_ascii_case("IXML") {
                                                // This is IXML content embedded as key=value
                                                ixml_content = Some(value.to_string());
                                            } else {
                                                // Regular text metadata
                                                text_tags.push(MetadataChunk::TextTag {
                                                    key: key.to_string(),
                                                    value: value.to_string(),
                                                });
                                            }
                                        } else if comment.contains("<BWFXML>")
                                            || comment.starts_with("iXML")
                                        {
                                            // This might be IXML content without a key prefix
                                            ixml_content = Some(comment);
                                        }
                                    } else {
                                        // Binary data that's not UTF-8 - check if it might be IXML
                                        let data_string = String::from_utf8_lossy(&comment_data);
                                        if data_string.contains("<BWFXML>")
                                            || data_string.starts_with("iXML")
                                        {
                                            ixml_content = Some(data_string.to_string());
                                            vorbis_comments
                                                .push(format!("IXML_BINARY_DATA={}", data_string));
                                        } else {
                                            // Store as unknown binary data
                                            use base64::{Engine as _, engine::general_purpose};
                                            vorbis_comments.push(format!(
                                                "BINARY_DATA={}",
                                                general_purpose::STANDARD.encode(&comment_data)
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        // Add IXML chunk if found
                        if let Some(ixml) = ixml_content {
                            chunks.push(MetadataChunk::IXml(ixml));
                        }

                        // Add text tags for all other metadata
                        chunks.extend(text_tags);

                        // Also preserve the raw Vorbis comment as an unknown chunk for completeness
                        // This ensures we don't lose any metadata during round-trip operations
                        if !vorbis_comments.is_empty() {
                            let vorbis_text = vorbis_comments.join("\n");
                            chunks.push(MetadataChunk::Unknown {
                                id: "FLAC_VORBIS_COMMENTS".to_string(),
                                data: vorbis_text.into_bytes(),
                            });
                        }
                    }
                }
                PICTURE_BLOCK_TYPE => {
                    // Parse picture metadata according to the FLAC/Vorbis spec
                    if data.len() > 32 {
                        let mut pic_cursor = Cursor::new(&data);

                        // Skip picture type (4 bytes)
                        pic_cursor.seek(SeekFrom::Current(4))?;

                        // Read MIME type length
                        let mime_len = pic_cursor.read_u32::<BigEndian>()? as usize;
                        if mime_len > 0 && mime_len < 128 {
                            let mut mime_bytes = vec![0u8; mime_len];
                            pic_cursor.read_exact(&mut mime_bytes)?;
                            let mime_type = String::from_utf8_lossy(&mime_bytes).to_string();

                            // Read description length
                            let desc_len = pic_cursor.read_u32::<BigEndian>()? as usize;
                            let mut desc_bytes = vec![0u8; desc_len];
                            pic_cursor.read_exact(&mut desc_bytes)?;
                            let description = String::from_utf8_lossy(&desc_bytes).to_string();

                            // Skip width, height, color depth, colors used (16 bytes)
                            pic_cursor.seek(SeekFrom::Current(16))?;

                            // Read picture data
                            let data_len = pic_cursor.read_u32::<BigEndian>()? as usize;
                            let mut pic_data = vec![0u8; data_len];
                            pic_cursor.read_exact(&mut pic_data)?;

                            chunks.push(MetadataChunk::Picture {
                                mime_type,
                                description,
                                data: pic_data,
                            });
                        }
                    } else {
                        // Fallback if parsing fails
                        chunks.push(MetadataChunk::Unknown {
                            id: "FLAC_PICTURE".to_string(),
                            data: data.to_vec(),
                        });
                    }
                }
                // Handle Soundminer and other application-specific metadata blocks
                1 => {
                    // APPLICATION block - check if it's Soundminer or contains IXML
                    if data.len() >= 4 {
                        // First 4 bytes should be the application ID
                        let app_id = &data[0..4];
                        if app_id == b"SMED" || app_id == b"smgz" {
                            // This is a Soundminer block, extract IXML content only
                            let content = String::from_utf8_lossy(&data[4..]);
                            if content.contains("<BWFXML>") || content.contains("iXML") {
                                // Extract IXML content - this is what you actually need
                                chunks.push(MetadataChunk::IXml(content.to_string()));
                            }
                            // Don't preserve the binary SMED chunk since it's not useful
                        } else {
                            // Other application block - check for IXML in non-Soundminer apps
                            let content = String::from_utf8_lossy(&data[4..]);
                            if content.contains("<BWFXML>") || content.contains("iXML") {
                                chunks.push(MetadataChunk::IXml(content.to_string()));
                            }
                            chunks.push(MetadataChunk::Unknown {
                                id: format!("FLAC_APPLICATION_{}", String::from_utf8_lossy(app_id)),
                                data,
                            });
                        }
                    } else {
                        chunks.push(MetadataChunk::Unknown {
                            id: "FLAC_APPLICATION".to_string(),
                            data,
                        });
                    }
                }
                // Add other metadata types as needed
                _ => {
                    // Check if the data contains IXML content regardless of block type
                    let data_string = String::from_utf8_lossy(&data);
                    if data_string.contains("<BWFXML>") || data_string.contains("iXML") {
                        chunks.push(MetadataChunk::IXml(data_string.to_string()));
                    }

                    chunks.push(MetadataChunk::Unknown {
                        id: format!("FLAC_{}", block_type),
                        data,
                    });
                }
            }
        }

        Ok(chunks)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Option<Metadata>) -> R<()> {
        let Some(metadata) = metadata else {
            return Err(anyhow!("Cannot embed None Metadata"));
        };
        let (source_tag, chunks) = match metadata {
            Metadata::Flac(tag, chunks) => (tag, chunks),
            _ => return Err(anyhow!("Unsupported metadata format")),
        };

        // First, handle the standard metaflac tags
        let mut dest_tags = Tag::read_from_path(file_path)?;
        for block in source_tag.blocks() {
            match block {
                Block::VorbisComment(_)
                | Block::Picture(_)
                | Block::CueSheet(_)
                | Block::Application(_)
                | Block::Unknown(_) => {
                    dest_tags.push_block(block.clone());
                }
                _ => {}
            }
        }
        dest_tags.save()?;

        // Then, handle the custom metadata chunks if any
        if !chunks.is_empty() {
            // Read the file that was just updated with metaflac tags
            let file_data = std::fs::read(file_path)?;

            // Use embed_metadata_chunks to add the custom chunks
            let updated_data = self.embed_metadata_chunks(&file_data, chunks)?;

            // Write the final result back to the file
            std::fs::write(file_path, updated_data)?;
        }

        Ok(())
    }

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>> {
        // Skip processing if there are no chunks to embed
        if chunks.is_empty() {
            return Ok(input.to_vec());
        }

        let mut cursor = Cursor::new(input);
        let mut output = Cursor::new(Vec::new());

        // Copy FLAC marker
        let mut marker = [0u8; 4];
        cursor.read_exact(&mut marker)?;
        output.write_all(&marker)?;

        // Read the STREAMINFO block
        let header = cursor.read_u8()?;
        let block_type = header & 0x7F;
        let is_last = (header & LAST_METADATA_BLOCK_FLAG) != 0;
        let block_size = cursor.read_u24::<BigEndian>()? as usize;

        if block_type != STREAMINFO_BLOCK_TYPE {
            return Err(anyhow!("First metadata block is not STREAMINFO"));
        }

        // Copy STREAMINFO data - never modify this
        let mut streaminfo_data = vec![0u8; block_size];
        cursor.read_exact(&mut streaminfo_data)?;

        // Write STREAMINFO without LAST flag (we're adding metadata)
        output.write_u8(STREAMINFO_BLOCK_TYPE)?;
        output.write_u24::<BigEndian>(block_size as u32)?;
        output.write_all(&streaminfo_data)?;

        // Skip any existing metadata blocks in the original file
        // We'll completely replace them with our new metadata
        let mut found_last = is_last;
        while !found_last {
            let header = cursor.read_u8()?;
            found_last = (header & LAST_METADATA_BLOCK_FLAG) != 0;
            let block_size = cursor.read_u24::<BigEndian>()? as usize;

            // Skip the block data
            cursor.seek(SeekFrom::Current(block_size as i64))?;
        }

        // Organize chunks into groups
        let mut text_tags = Vec::new();
        let mut ixml_chunks = Vec::new();
        let mut picture_chunks = Vec::new();
        let mut other_chunks = Vec::new();

        for chunk in chunks {
            match chunk {
                MetadataChunk::IXml(_) => ixml_chunks.push(chunk),
                MetadataChunk::Picture { .. } => picture_chunks.push(chunk),
                MetadataChunk::TextTag { .. } => text_tags.push(chunk),
                _ => other_chunks.push(chunk),
            }
        }

        // Collect all blocks to write
        let mut blocks_to_write = Vec::new();

        // Create unified Vorbis comment block if we have text data or IXML
        if !text_tags.is_empty() || !ixml_chunks.is_empty() {
            let mut vorbis_data = Cursor::new(Vec::new());

            // Vendor string
            let vendor = b"FFCodex";
            vorbis_data.write_u32::<LittleEndian>(vendor.len() as u32)?;
            vorbis_data.write_all(vendor)?;

            // Collect all comments
            let mut comments = Vec::new();

            // Add IXML as IXML= comments
            for chunk in &ixml_chunks {
                if let MetadataChunk::IXml(ixml_content) = chunk {
                    comments.push(format!("IXML={}", ixml_content));
                }
            }

            // Add text tags
            for chunk in &text_tags {
                if let MetadataChunk::TextTag { key, value } = chunk {
                    comments.push(format!("{}={}", key, value));
                }
            }

            // Write comment count and comments
            vorbis_data.write_u32::<LittleEndian>(comments.len() as u32)?;
            for comment in comments {
                let comment_bytes = comment.as_bytes();
                vorbis_data.write_u32::<LittleEndian>(comment_bytes.len() as u32)?;
                vorbis_data.write_all(comment_bytes)?;
            }

            blocks_to_write.push((VORBIS_COMMENT_BLOCK_TYPE, vorbis_data.into_inner()));
        }

        // Add picture blocks
        for chunk in &picture_chunks {
            if let MetadataChunk::Picture {
                mime_type,
                description,
                data,
            } = chunk
            {
                let mut pic_data = Cursor::new(Vec::new());

                // Picture type (0 = Other)
                pic_data.write_u32::<BigEndian>(0)?;

                // MIME type
                pic_data.write_u32::<BigEndian>(mime_type.len() as u32)?;
                pic_data.write_all(mime_type.as_bytes())?;

                // Description
                pic_data.write_u32::<BigEndian>(description.len() as u32)?;
                pic_data.write_all(description.as_bytes())?;

                // Width, height, color depth, colors used (all 0 for unknown)
                pic_data.write_u32::<BigEndian>(0)?;
                pic_data.write_u32::<BigEndian>(0)?;
                pic_data.write_u32::<BigEndian>(0)?;
                pic_data.write_u32::<BigEndian>(0)?;

                // Picture data
                pic_data.write_u32::<BigEndian>(data.len() as u32)?;
                pic_data.write_all(data)?;

                blocks_to_write.push((PICTURE_BLOCK_TYPE, pic_data.into_inner()));
            }
        }

        // Add other chunks (application blocks, etc.)
        for chunk in &other_chunks {
            if let MetadataChunk::Unknown { id, data } = chunk {
                let block_type = if id.starts_with("FLAC_") {
                    // Try to parse the block type from the ID
                    if let Ok(parsed_type) = id.trim_start_matches("FLAC_").parse::<u8>() {
                        parsed_type & 0x7F
                    } else {
                        // Use application block type for unknown blocks
                        0x7F
                    }
                } else {
                    // Default to application block
                    0x7F
                };
                blocks_to_write.push((block_type, data.clone()));
            }
        }

        // Write all metadata blocks
        for (i, (block_type, data)) in blocks_to_write.iter().enumerate() {
            let is_last_block = i == blocks_to_write.len() - 1;

            let header = if is_last_block {
                block_type | LAST_METADATA_BLOCK_FLAG
            } else {
                *block_type
            };

            output.write_u8(header)?;
            output.write_u24::<BigEndian>(data.len() as u32)?;
            output.write_all(data)?;
        }

        // Copy the remaining audio data
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
