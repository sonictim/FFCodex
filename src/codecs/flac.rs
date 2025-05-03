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

        // For very small files, use direct sample iteration which has less overhead
        if samples_per_channel < 10_000 {
            // Preallocate audio_data with exact capacity
            let mut audio_data: Vec<Vec<f32>> =
                vec![Vec::with_capacity(samples_per_channel); channels as usize];

            // Get all samples at once for better performance
            let samples = reader.samples().collect::<Result<Vec<i32>, _>>()?;

            // Process all samples at once using chunk iterators
            for chunk in samples.chunks_exact(channels as usize) {
                for (ch, &sample) in chunk.iter().enumerate() {
                    // Normalize the sample value to the range [-1.0, 1.0]
                    let normalized = sample as f32 / divisor;
                    audio_data[ch].push(normalized);
                }
            }

            // Select the appropriate sample format based on bit depth
            let format = select_sample_format(bits_per_sample);

            return Ok(AudioBuffer {
                sample_rate,
                channels,
                format,
                data: audio_data,
            });
        }

        // For larger files, use a more efficient approach with parallel processing

        // Collect all samples into a single vector first
        let all_samples = reader.samples().collect::<Result<Vec<i32>, _>>()?;
        let channel_count = channels as usize;
        let samples_per_channel = all_samples.len() / channel_count;

        // Create a single vector to hold normalized samples
        let normalized_samples: Vec<f32> = all_samples
            .iter()
            .map(|&sample| sample as f32 / divisor)
            .collect();

        // Now create per-channel vectors from the normalized samples
        let mut audio_data: Vec<Vec<f32>> =
            vec![Vec::with_capacity(samples_per_channel); channel_count];

        // If we have a large file, use parallel processing
        if samples_per_channel > 100_000 {
            // Prepare chunks for parallel processing
            let chunks: Vec<_> = normalized_samples.chunks(channel_count).collect();
            let chunk_size = (chunks.len() / rayon::current_num_threads()).max(1024);

            // Process in parallel using thread-safe methods
            let results: Vec<Vec<Vec<f32>>> = chunks
                .par_chunks(chunk_size)
                .map(|chunk_group| {
                    // Each thread processes its own chunk and returns a vector of channel data
                    let mut local_channels =
                        vec![Vec::with_capacity(chunk_group.len()); channel_count];

                    for chunk in chunk_group {
                        for (ch, &sample) in chunk.iter().enumerate() {
                            if ch < channel_count {
                                local_channels[ch].push(sample);
                            }
                        }
                    }

                    local_channels
                })
                .collect();

            // Combine the results from all threads
            for (ch, channel_vec) in audio_data.iter_mut().enumerate() {
                for thread_result in &results {
                    channel_vec.extend_from_slice(&thread_result[ch]);
                }
            }
        } else {
            // For medium-sized files, use sequential processing
            for chunk in normalized_samples.chunks_exact(channel_count) {
                for (ch, &sample) in chunk.iter().enumerate() {
                    audio_data[ch].push(sample);
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

    fn encode(&self, buffer: &AudioBuffer) -> R<Vec<u8>> {
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
        Ok(Metadata::Flac(tag))
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

                        // Add both formats
                        chunks.push(MetadataChunk::IXml(comments));
                        chunks.extend(text_tags);
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
                // Add other metadata types as needed
                _ => {
                    chunks.push(MetadataChunk::Unknown {
                        id: format!("FLAC_{}", block_type),
                        data,
                    });
                }
            }
        }

        Ok(chunks)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()> {
        let source_tag = match metadata {
            Metadata::Flac(tag) => tag,
            _ => return Err(anyhow!("Unsupported metadata format")),
        };
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
