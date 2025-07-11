use crate::prelude::*;

// Format tags
const FORMAT_PCM: u16 = 1;
const FORMAT_IEEE_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 65534; // 0xFFFE

// Chunk Identifiers
const RIFF_CHUNK_ID: &[u8; 4] = b"RIFF";
const WAVE_FORMAT_ID: &[u8; 4] = b"WAVE";
const FMT_CHUNK_ID: &[u8; 4] = b"fmt ";
const DATA_CHUNK_ID: &[u8; 4] = b"data";

// Chunk Structures
const STANDARD_FMT_CHUNK_SIZE: u32 = 16;
const HEADER_SIZE: usize = 12; // RIFF + size + WAVE

pub struct WavCodec;

impl Codec for WavCodec {
    fn file_extension(&self) -> &'static str {
        "wav"
    }
    fn validate_file_format(&self, data: &[u8]) -> R<()> {
        // Check file size
        if data.len() < HEADER_SIZE {
            return Err(anyhow!("File too small to be a valid WAV"));
        }

        // Check for 'RIFF....WAVE' header
        if &data[0..4] != RIFF_CHUNK_ID || &data[8..12] != WAVE_FORMAT_ID {
            return Err(anyhow!("Invalid WAV File: Missing RIFF/WAVE signature"));
        }

        Ok(())
    }
    fn get_file_info(&self, file_path: &str) -> R<FileInfo> {
        use byteorder::{LittleEndian, ReadBytesExt};
        use std::fs::metadata;
        use std::io::{Cursor, Read, Seek, SeekFrom};

        // Get file size
        let file_metadata = metadata(file_path)?;
        let file_size = file_metadata.len() as usize;

        // Open and map the file
        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        // Validate file format
        self.validate_file_format(&mapped_file)?;

        let mut cursor = Cursor::new(&*mapped_file);

        // Skip RIFF header (12 bytes)
        cursor.seek(SeekFrom::Start(HEADER_SIZE as u64))?;

        // Find and parse the fmt chunk
        let mut sample_rate = 0u32;
        let mut channels = 0u16;
        let mut bits_per_sample = 0u16;
        let mut data_size = 0u32;

        // Description candidates in priority order
        let mut bext_description = String::new();
        let mut ixml_user_description = String::new();
        let mut ixml_bext_bwf_description = String::new();
        let mut id3_comment = String::new();

        while cursor.position() < mapped_file.len() as u64 {
            let mut chunk_id = [0u8; 4];
            if cursor.read(&mut chunk_id)? < 4 {
                break;
            }

            let chunk_size = cursor.read_u32::<LittleEndian>()?;

            match &chunk_id {
                FMT_CHUNK_ID => {
                    if chunk_size >= 16 {
                        let _format_tag = cursor.read_u16::<LittleEndian>()?;
                        channels = cursor.read_u16::<LittleEndian>()?;
                        sample_rate = cursor.read_u32::<LittleEndian>()?;
                        cursor.read_u32::<LittleEndian>()?; // byte rate
                        cursor.read_u16::<LittleEndian>()?; // block align
                        bits_per_sample = cursor.read_u16::<LittleEndian>()?;

                        // Skip any extra bytes in the fmt chunk
                        let extra_bytes = if chunk_size > 16 { chunk_size - 16 } else { 0 };
                        cursor.seek(SeekFrom::Current(extra_bytes as i64))?;
                    }
                }
                DATA_CHUNK_ID => {
                    data_size = chunk_size;
                    // Don't read the data, just skip it
                    cursor.seek(SeekFrom::Current(chunk_size as i64))?;
                }
                b"bext" => {
                    // Priority 1: BWF Broadcast Extension chunk "Description" field
                    if chunk_size >= 256 {
                        let mut bext_data = vec![0u8; 256];
                        cursor.read_exact(&mut bext_data)?;
                        bext_description = String::from_utf8_lossy(&bext_data)
                            .trim_end_matches('\0')
                            .trim()
                            .to_string();

                        // Skip remaining bext data
                        if chunk_size > 256 {
                            cursor.seek(SeekFrom::Current((chunk_size - 256) as i64))?;
                        }
                    } else {
                        cursor.seek(SeekFrom::Current(chunk_size as i64))?;
                    }
                }
                b"iXML" => {
                    // Priority 2 & 3: iXML chunk - look for USER_DESCRIPTION and BEXT_BWF_DESCRIPTION
                    let mut xml_data = vec![0u8; chunk_size as usize];
                    cursor.read_exact(&mut xml_data)?;
                    let xml_string = String::from_utf8_lossy(&xml_data);

                    // Parse iXML for specific fields
                    for line in xml_string.lines() {
                        if let Some(idx) = line.find('=') {
                            let key = line[0..idx].trim();
                            let value = line[idx + 1..].trim().to_string();

                            match key {
                                "USER_DESCRIPTION" => {
                                    if !value.is_empty() {
                                        ixml_user_description = value;
                                    }
                                }
                                "BEXT_BWF_DESCRIPTION" => {
                                    if !value.is_empty() {
                                        ixml_bext_bwf_description = value;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                b"ID3 " | b"id3 " => {
                    // Priority 4: ID3 chunk - look for Comment
                    let mut id3_data = vec![0u8; chunk_size as usize];
                    cursor.read_exact(&mut id3_data)?;
                    id3_comment = extract_id3_comment(&id3_data);
                }
                _ => {
                    // Skip other chunks
                    cursor.seek(SeekFrom::Current(chunk_size as i64))?;
                }
            }

            // Handle padding
            if chunk_size % 2 == 1 {
                cursor.seek(SeekFrom::Current(1))?;
            }
        }

        // Select description based on priority order
        let description = if !bext_description.is_empty() {
            bext_description
        } else if !ixml_user_description.is_empty() {
            ixml_user_description
        } else if !ixml_bext_bwf_description.is_empty() {
            ixml_bext_bwf_description
        } else if !id3_comment.is_empty() {
            id3_comment
        } else {
            String::new()
        };

        // Calculate duration
        let duration = if sample_rate > 0 && channels > 0 && bits_per_sample > 0 {
            let bytes_per_sample = bits_per_sample / 8;
            let bytes_per_second = sample_rate * channels as u32 * bytes_per_sample as u32;
            let duration_seconds = data_size as f64 / bytes_per_second as f64;

            let hours = (duration_seconds / 3600.0) as u32;
            let minutes = ((duration_seconds % 3600.0) / 60.0) as u32;
            let seconds = (duration_seconds % 60.0) as u32;
            let milliseconds = ((duration_seconds % 1.0) * 1000.0) as u32;

            if hours > 0 {
                format!(
                    "{}:{:02}:{:02}.{:03}",
                    hours, minutes, seconds, milliseconds
                )
            } else {
                format!("{}:{:02}.{:03}", minutes, seconds, milliseconds)
            }
        } else {
            "Unknown".to_string()
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
        self.validate_file_format(input)?;

        // Additional file size validation
        if input.len() < 44 {
            // Minimum size for a valid WAV file
            return Err(anyhow!("WAV file too small: {} bytes", input.len()));
        }

        let mut cursor = Cursor::new(input);

        // Skip past the RIFF header we already validated (12 bytes)
        cursor.seek(SeekFrom::Start(HEADER_SIZE as u64))?;

        // Step 2: Find 'fmt ' chunk
        let mut fmt_chunk_found = false;
        let mut data_chunk_found = false;
        let mut sample_format = SampleFormat::I16;
        let mut channels = 0;
        let mut sample_rate = 0;
        let mut bits_per_sample = 0;
        let mut audio_data = vec![];

        while let Ok(chunk_id) = cursor.read_u32::<LittleEndian>() {
            let chunk_id = u32::to_le_bytes(chunk_id);
            let chunk_size = cursor.read_u32::<LittleEndian>()? as usize;
            match &chunk_id {
                FMT_CHUNK_ID => {
                    fmt_chunk_found = true;
                    let format_tag = cursor.read_u16::<LittleEndian>()?;
                    channels = cursor.read_u16::<LittleEndian>()?;
                    dprintln!("Decode Channels: {}", channels);
                    sample_rate = cursor.read_u32::<LittleEndian>()?;
                    cursor.read_u32::<LittleEndian>()?; // byte rate
                    cursor.read_u16::<LittleEndian>()?; // block align
                    bits_per_sample = cursor.read_u16::<LittleEndian>()?;

                    sample_format = match (format_tag, bits_per_sample) {
                        (FORMAT_PCM, BIT_DEPTH_8) => SampleFormat::U8,
                        (FORMAT_PCM, BIT_DEPTH_16) => SampleFormat::I16,
                        (FORMAT_PCM, BIT_DEPTH_24) => SampleFormat::I24,
                        (FORMAT_PCM, BIT_DEPTH_32) => SampleFormat::I32,
                        (FORMAT_IEEE_FLOAT, BIT_DEPTH_32) => SampleFormat::F32,
                        (FORMAT_EXTENSIBLE, bits) => {
                            // For WAVE_FORMAT_EXTENSIBLE, we need to read the extended format data
                            // The format is a 22-byte structure after the standard fmt chunk

                            // First, read the extension size (should be 22 for extensible format)
                            let extension_size = cursor.read_u16::<LittleEndian>()?;

                            // Track bytes already read for EXTENSIBLE format
                            let mut bytes_read = 2; // 2 bytes for extension_size

                            // Read the valid bits per sample (may be different from container size)
                            let _valid_bits = cursor.read_u16::<LittleEndian>()?;
                            bytes_read += 2;

                            // Read the channel mask (indicates speaker positions)
                            let _channel_mask = cursor.read_u32::<LittleEndian>()?;
                            bytes_read += 4;

                            // Read the subformat GUID (first 2 bytes are the actual format code)
                            let mut guid = [0u8; 16];
                            cursor.read_exact(&mut guid)?;
                            bytes_read += 16;

                            // The first two bytes of the GUID indicate the actual format
                            let subformat = u16::from_le_bytes([guid[0], guid[1]]);

                            // Return the correct format for the subformat
                            let format = match (subformat, bits) {
                                (FORMAT_PCM, BIT_DEPTH_8) => SampleFormat::U8,
                                (FORMAT_PCM, BIT_DEPTH_16) => SampleFormat::I16,
                                (FORMAT_PCM, BIT_DEPTH_24) => SampleFormat::I24,
                                (FORMAT_PCM, BIT_DEPTH_32) => SampleFormat::I32,
                                (FORMAT_IEEE_FLOAT, BIT_DEPTH_32) => SampleFormat::F32,
                                _ => {
                                    return Err(anyhow!(format!(
                                        "Unsupported extensible format: subformat {}, bits {}",
                                        subformat, bits
                                    )));
                                }
                            };

                            // Check if there are more bytes in the extension that we need to skip
                            if extension_size > bytes_read {
                                cursor.seek(SeekFrom::Current(
                                    (extension_size - bytes_read) as i64,
                                ))?;
                            }

                            format
                        }
                        _ => {
                            return Err(anyhow!(format!(
                                "Unsupported format: tag {}, bits {}",
                                format_tag, bits_per_sample
                            )));
                        }
                    };

                    // Skip any extra bytes in the fmt chunk and handle padding in one operation
                    // Only skip extra bytes if we're not in the EXTENSIBLE format case, since we've already handled those bytes
                    let extra_bytes = if chunk_size > STANDARD_FMT_CHUNK_SIZE as usize
                        && format_tag != FORMAT_EXTENSIBLE
                    {
                        chunk_size - STANDARD_FMT_CHUNK_SIZE as usize
                    } else if format_tag == FORMAT_EXTENSIBLE {
                        // For EXTENSIBLE format, we've already read the extension data above
                        0
                    } else {
                        0
                    };

                    let padding_byte = chunk_size % 2;
                    cursor.seek(SeekFrom::Current((extra_bytes + padding_byte) as i64))?;
                }

                DATA_CHUNK_ID => {
                    data_chunk_found = true;
                    let mut raw_data = vec![0u8; chunk_size];
                    cursor.read_exact(&mut raw_data)?;

                    audio_data = decode_samples(
                        &raw_data,
                        channels,
                        bits_per_sample,
                        sample_format == SampleFormat::F32,
                    )?;

                    // Handle padding in one step
                    if chunk_size % 2 != 0 {
                        cursor.seek(SeekFrom::Current(1))?;
                    }
                }

                _ => {
                    // Skip chunk data and padding in one operation
                    let skip_bytes = chunk_size + (chunk_size % 2);
                    cursor.seek(SeekFrom::Current(skip_bytes as i64))?;
                }
            }
        }

        if !fmt_chunk_found || !data_chunk_found {
            return Err(anyhow!("Missing 'fmt ' or 'data' chunk"));
        }

        Ok(AudioBuffer {
            sample_rate,
            channels,
            format: sample_format,
            data: audio_data,
        })
    }

    fn encode(&self, buffer: &Option<AudioBuffer>) -> R<Vec<u8>> {
        let Some(buffer) = buffer else {
            return Err(anyhow!("Cannot encode None AudioBuffer"));
        };
        let mut output = Cursor::new(Vec::new());

        // Ensure channel count in buffer is consistent with data
        let actual_channels = buffer.data.len() as u16;
        let channels = if actual_channels != buffer.channels {
            actual_channels
        } else {
            buffer.channels
        };

        // Placeholder for header
        output.write_all(RIFF_CHUNK_ID)?;
        output.write_u32::<LittleEndian>(0)?; // placeholder file size
        output.write_all(WAVE_FORMAT_ID)?;

        // ---- fmt chunk ----
        output.write_all(FMT_CHUNK_ID)?;
        output.write_u32::<LittleEndian>(STANDARD_FMT_CHUNK_SIZE)?; // PCM = 16 bytes
        let (format_tag, bits_per_sample) = match buffer.format {
            SampleFormat::F32 => (FORMAT_IEEE_FLOAT, BIT_DEPTH_32),
            SampleFormat::I16 => (FORMAT_PCM, BIT_DEPTH_16),
            SampleFormat::I24 => (FORMAT_PCM, BIT_DEPTH_24),
            SampleFormat::I32 => (FORMAT_PCM, BIT_DEPTH_32),
            SampleFormat::U8 => (FORMAT_PCM, BIT_DEPTH_8),
        };
        let sample_rate = buffer.sample_rate;
        let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
        let block_align = channels * bits_per_sample / 8;

        output.write_u16::<LittleEndian>(format_tag)?;
        output.write_u16::<LittleEndian>(channels)?; // Use the verified channel count
        output.write_u32::<LittleEndian>(sample_rate)?;
        output.write_u32::<LittleEndian>(byte_rate)?;
        output.write_u16::<LittleEndian>(block_align)?;
        output.write_u16::<LittleEndian>(bits_per_sample)?;

        // ---- data chunk ----
        output.write_all(DATA_CHUNK_ID)?;
        let data_pos = output.position();
        output.write_u32::<LittleEndian>(0)?; // placeholder

        let start_data = output.position();

        let mut interleaved_bytes = Vec::new();
        encode_samples(&mut interleaved_bytes, buffer, bits_per_sample)?;

        output.write_all(&interleaved_bytes)?;

        let end_data = output.position();
        let data_size = (end_data - start_data) as u32;

        // Fill in data chunk size
        let mut out = output.into_inner();
        (&mut out[(data_pos as usize)..(data_pos as usize + 4)])
            .write_u32::<LittleEndian>(data_size)?;

        // Fill in RIFF file size
        let riff_size = out.len() as u32 - 8;
        (&mut out[4..8]).write_u32::<LittleEndian>(riff_size)?;

        Ok(out)
    }

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata> {
        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        // Let's check the channel count in the WAV header before extraction
        if file_path.ends_with(".wav") {
            let mut cursor = Cursor::new(&mapped_file);

            // Read RIFF header first to validate
            let mut header = [0u8; 12];
            if cursor.read_exact(&mut header).is_ok() {
                if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
                    // Not a valid WAVE file
                } else {
                    // Look for the fmt chunk
                    while cursor.position() < mapped_file.len() as u64 {
                        let mut chunk_id = [0u8; 4];
                        if cursor.read(&mut chunk_id)? < 4 {
                            break;
                        }

                        let chunk_size = cursor.read_u32::<LittleEndian>()?;

                        if &chunk_id == FMT_CHUNK_ID {
                            // Found fmt chunk
                            if chunk_size >= 16 {
                                // Ensure fmt chunk is at least standard size
                                // Format type
                                let _format_tag = cursor.read_u16::<LittleEndian>()?;
                                // Channel count is right after format tag
                                let channel_count = cursor.read_u16::<LittleEndian>()?;

                                // Validate the channel count
                                if !(1..=128).contains(&channel_count) {
                                    // Suspicious channel count
                                }

                                // Get sample rate while we're at it
                                let _sample_rate = cursor.read_u32::<LittleEndian>()?;

                                // Don't need to read further in fmt chunk
                                break;
                            }
                        } else {
                            // Skip this chunk
                            cursor.seek(SeekFrom::Current(
                                chunk_size as i64 + (chunk_size % 2) as i64,
                            ))?;
                        }
                    }
                }
            }
        }

        let chunks = self.extract_metadata_chunks(&mapped_file)?;
        dprintln!(
            "extract_file_metadata_chunks - Found {} metadata chunks",
            chunks.len()
        );
        Ok(Metadata::Wav(chunks))
    }

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>> {
        let mut cursor = Cursor::new(input);

        let mut header = [0u8; 12];
        cursor.read_exact(&mut header)?;

        if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            return Err(anyhow!("Not a WAV file"));
        }

        let mut chunks = Vec::new();
        while cursor.position() < input.len() as u64 {
            let mut id = [0u8; 4];
            if cursor.read(&mut id)? < 4 {
                break;
            }

            let size = cursor.read_u32::<LittleEndian>()?;

            // Skip the 'data' chunk and 'fmt ' chunk - they're not metadata
            if &id == DATA_CHUNK_ID || &id == FMT_CHUNK_ID {
                cursor.seek(SeekFrom::Current(size as i64 + (size % 2) as i64))?;
                continue;
            }

            let mut data = vec![0u8; size as usize];
            cursor.read_exact(&mut data)?;

            let chunk = match &id {
                b"bext" => MetadataChunk::Bext(data),
                b"iXML" => {
                    let xml = String::from_utf8_lossy(&data).to_string();

                    // Also extract individual text tags for better format conversion
                    for line in xml.lines() {
                        if let Some(idx) = line.find('=') {
                            let key = line[0..idx].trim().to_string();
                            let value = line[idx + 1..].trim().to_string();

                            // Only add if it's a valid key-value pair
                            if !key.is_empty() {
                                chunks.push(MetadataChunk::TextTag { key, value });
                            }
                        }
                    }

                    MetadataChunk::IXml(xml)
                }
                // Recognize ID3 chunk if present in WAV
                b"id3 " | b"ID3 " => MetadataChunk::ID3(data),
                // Picture/album art in WAV
                b"APIC" => {
                    // Try to extract picture metadata
                    if data.len() > 8 {
                        // Simple picture extraction
                        // In a real implementation, you'd parse the APIC structure properly
                        chunks.push(MetadataChunk::Picture {
                            mime_type: "image/jpeg".to_string(), // Default assumption
                            description: "Album Art".to_string(),
                            data: data.clone(),
                        });
                    }

                    // Also keep the raw data
                    MetadataChunk::Unknown {
                        id: "APIC".to_string(),
                        data,
                    }
                }
                b"SMED" | b"SMRD" | b"SMPL" => MetadataChunk::Soundminer(data),
                _ => MetadataChunk::Unknown {
                    id: String::from_utf8_lossy(&id).to_string(),
                    data,
                },
            };

            chunks.push(chunk);

            // Padding: chunks are aligned to even sizes
            if size % 2 == 1 {
                cursor.seek(SeekFrom::Current(1))?;
            }
        }

        Ok(chunks)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Option<Metadata>) -> R<()> {
        let Some(metadata) = metadata else {
            return Err(anyhow!("No metadata provided for embedding"));
        };
        let chunks = match metadata {
            Metadata::Wav(chunks) => chunks,
            _ => return Err(anyhow!("Unsupported metadata format")),
        };

        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        // Use mapped_file as &[u8] without loading into memory
        let new_data = self.embed_metadata_chunks(&mapped_file, chunks)?;

        // Format-specific validation - only run for WAV files
        if file_path.ends_with(".wav") {
            let mut cursor = Cursor::new(&new_data);
            cursor.seek(SeekFrom::Start(22))?; // Position of channel count in WAV header
            let channel_count = cursor.read_u16::<LittleEndian>()?;
            dprintln!(
                "embed_file_metadata_chunks - Channel count in output file: {}",
                channel_count
            );
        }

        // Write the data back to the file
        std::fs::write(file_path, new_data)?;
        Ok(())
    }

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut output = Cursor::new(Vec::new());

        // Copy the RIFF/WAVE header
        let mut riff_header = [0u8; 12];
        cursor.read_exact(&mut riff_header)?;
        output.write_all(&riff_header)?;

        // Read the original channel count from the input file
        let mut original_cursor = Cursor::new(input);
        original_cursor.seek(SeekFrom::Start(22))?; // Position of channel count in WAV header
        let original_channels = original_cursor.read_u16::<LittleEndian>()?;

        // Group metadata by type for better organization
        let mut bext_chunks = Vec::new();
        let mut ixml_chunks = Vec::new();
        let mut picture_chunks = Vec::new();
        let mut id3_chunks = Vec::new();
        let mut text_tags = Vec::new();
        let mut other_chunks = Vec::new();

        // When reading and writing non-metadata chunks, preserve the original fmt chunk
        let mut fmt_chunk_found = false;

        // First collect all chunks from source audio
        while cursor.position() < input.len() as u64 {
            let mut id = [0u8; 4];
            if cursor.read(&mut id)? < 4 {
                break;
            }

            let size = cursor.read_u32::<LittleEndian>()?;
            let mut data = vec![0u8; size as usize];
            cursor.read_exact(&mut data)?;

            let id_str = String::from_utf8_lossy(&id).to_string();

            // Handle fmt chunk specially to preserve channel count
            if &id == FMT_CHUNK_ID {
                fmt_chunk_found = true;

                // We need to preserve the fmt chunk but ensure it has the correct channel count
                if original_channels == 1 {
                    // For mono files, make sure fmt chunk shows 1 channel
                    // Channel count is at offset 2 in fmt chunk
                    data[2] = 1;
                    data[3] = 0; // Little-endian representation of 1

                    // Update block align and byte rate to match mono format
                    let bits_per_sample = u16::from_le_bytes([data[14], data[15]]);
                    let bytes_per_sample = bits_per_sample / 8;

                    // Block align (offset 12-13) = channels * bytes_per_sample
                    let block_align = bytes_per_sample;
                    data[12] = (block_align & 0xFF) as u8;
                    data[13] = ((block_align >> 8) & 0xFF) as u8;

                    // Byte rate (offset 8-11) = sample_rate * block_align
                    let sample_rate = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                    let byte_rate = sample_rate * block_align as u32;
                    data[8] = (byte_rate & 0xFF) as u8;
                    data[9] = ((byte_rate >> 8) & 0xFF) as u8;
                    data[10] = ((byte_rate >> 16) & 0xFF) as u8;
                    data[11] = ((byte_rate >> 24) & 0xFF) as u8;
                }

                // Write the fmt chunk with potentially updated data
                output.write_all(&id)?;
                output.write_u32::<LittleEndian>(size)?;
                output.write_all(&data)?;

                if size % 2 == 1 {
                    output.write_all(&[0])?; // padding
                }
                continue;
            }

            // Skip known metadata chunks since we'll replace them
            if matches!(
                id_str.as_str(),
                "bext" | "iXML" | "SMED" | "SMRD" | "SMPL" | "id3 " | "ID3 " | "APIC"
            ) {
                if size % 2 == 1 {
                    cursor.seek(SeekFrom::Current(1))?;
                }
                continue;
            }

            // Write other chunks directly to output
            output.write_all(&id)?;
            output.write_u32::<LittleEndian>(size)?;
            output.write_all(&data)?;

            if size % 2 == 1 {
                output.write_all(&[0])?;
            }
        }

        // If the fmt chunk wasn't found in the input file (unlikely), don't proceed
        if !fmt_chunk_found {
            return Err(anyhow!("WAV file missing fmt chunk"));
        }

        // Organize metadata chunks by type
        for chunk in chunks {
            match chunk {
                MetadataChunk::Bext(data) => {
                    // Update channel count in Broadcast WAV extension if necessary
                    let mut bext_data = data.clone();
                    if original_channels == 1 && bext_data.len() >= 356 {
                        // Update channel count in BEXT chunk (at offset 354-355)
                        bext_data[354] = 1;
                        bext_data[355] = 0; // Little-endian representation of 1
                    }
                    bext_chunks.push(MetadataChunk::Bext(bext_data));
                }
                MetadataChunk::IXml(xml) => {
                    // Check for any channel references in iXML that need updating
                    let mut updated_xml = xml.clone();
                    if original_channels == 1 {
                        // Replace any references to "2 channels" or similar with "1 channel"
                        // This is a simplistic approach and might need refinement
                        updated_xml = updated_xml.replace("CHANNELS=2", "CHANNELS=1");
                        updated_xml = updated_xml.replace("channels=2", "channels=1");
                        updated_xml = updated_xml.replace("NumChannels=2", "NumChannels=1");
                    }
                    ixml_chunks.push(MetadataChunk::IXml(updated_xml));
                }
                MetadataChunk::Picture {
                    mime_type,
                    description,
                    data,
                } => picture_chunks.push(MetadataChunk::Picture {
                    mime_type: mime_type.clone(),
                    description: description.clone(),
                    data: data.clone(),
                }),
                MetadataChunk::ID3(data) => id3_chunks.push(MetadataChunk::ID3(data.clone())),
                MetadataChunk::TextTag { key, value } => text_tags.push(MetadataChunk::TextTag {
                    key: key.clone(),
                    value: value.clone(),
                }),
                MetadataChunk::APE(data) => {
                    // APE tags can be handled similarly to ID3
                    other_chunks.push(MetadataChunk::APE(data.clone()));
                }
                MetadataChunk::Soundminer(data) => {
                    other_chunks.push(MetadataChunk::Soundminer(data.clone()))
                }
                MetadataChunk::Unknown { id, data } => other_chunks.push(MetadataChunk::Unknown {
                    id: id.clone(),
                    data: data.clone(),
                }),
            }
        }

        // Consolidate text tags into iXML if no iXML chunk exists
        if ixml_chunks.is_empty() && !text_tags.is_empty() {
            let mut xml = String::new();
            for tag in &text_tags {
                if let MetadataChunk::TextTag { key, value } = tag {
                    xml.push_str(&format!("{}={}\n", key, value));
                }
            }
            if !xml.is_empty() {
                // Create an owned MetadataChunk that's stored directly in the vector
                ixml_chunks.push(MetadataChunk::IXml(xml));
            }
        }

        // Write metadata chunks in order
        // Write bext chunks
        for chunk in &bext_chunks {
            if let MetadataChunk::Bext(data) = chunk {
                write_chunk(&mut output, b"bext", data)?;
            }
        }

        // Write iXML chunks
        for chunk in &ixml_chunks {
            if let MetadataChunk::IXml(xml) = chunk {
                write_chunk(&mut output, b"iXML", xml.as_bytes())?;
            }
        }

        // Write picture chunks
        for chunk in &picture_chunks {
            if let MetadataChunk::Picture { data, .. } = chunk {
                // In WAV, we need to use a custom chunk for pictures
                write_chunk(&mut output, b"APIC", data)?;
            }
        }

        // Write ID3 chunks
        for chunk in &id3_chunks {
            if let MetadataChunk::ID3(data) = chunk {
                write_chunk(&mut output, b"id3 ", data)?;
            }
        }

        // Write Soundminer and other chunks
        for chunk in &other_chunks {
            match chunk {
                MetadataChunk::Soundminer(data) => write_chunk(&mut output, b"SMED", data)?,
                MetadataChunk::Unknown { id, data } => {
                    write_chunk(&mut output, id.as_bytes(), data)?;
                }
                _ => {} // Skip other types we don't handle
            }
        }

        // Update RIFF chunk size
        let final_size = output.position() as u32 - 8;
        let output_data = output.into_inner();
        let mut result_data = output_data.clone();
        (&mut result_data[4..8]).write_u32::<LittleEndian>(final_size)?;

        Ok(result_data)
    }
}

fn decode_samples(
    input: &[u8],
    channels: u16,
    bits_per_sample: u16,
    is_float_format: bool,
) -> R<Vec<Vec<f32>>> {
    let bytes_per_sample = match bits_per_sample {
        BIT_DEPTH_8 => 1,
        BIT_DEPTH_16 => 2,
        BIT_DEPTH_24 => 3,
        BIT_DEPTH_32 => 4,
        _ => return Err(anyhow!("Unsupported bit depth")),
    };

    // Total frame count = total bytes / (bytes per sample * channel count)
    let frame_count = input.len() / (bytes_per_sample * channels as usize);

    // Ensure we have enough data
    if frame_count == 0 {
        return Err(anyhow!("No audio frames found in data"));
    }

    dprintln!(
        "Decoding {} channels, {} frames per channel, {} bits per sample",
        channels,
        frame_count,
        bits_per_sample
    );

    let output: Vec<Vec<f32>> = (0..channels as usize)
        .into_par_iter() // Parallelize over channels
        .map(|ch| {
            let mut channel_data = Vec::with_capacity(frame_count);

            // For each frame
            for frame in 0..frame_count {
                // Calculate the byte index of this sample
                // This is the formula that properly handles interleaved audio of any channel count
                let sample_idx = (frame * channels as usize + ch) * bytes_per_sample;

                // Check bounds to prevent buffer overruns
                if sample_idx + bytes_per_sample > input.len() {
                    break;
                }

                // Convert the bytes to a float sample
                let val = match bits_per_sample {
                    BIT_DEPTH_8 => input[sample_idx] as f32 / U8_SCALE - 1.0,
                    BIT_DEPTH_16 => {
                        let val = i16::from_le_bytes([input[sample_idx], input[sample_idx + 1]]);
                        val as f32 / I16_DIVISOR
                    }
                    BIT_DEPTH_24 => {
                        let val = ((input[sample_idx + 2] as i32) << 16)
                            | ((input[sample_idx + 1] as i32) << 8)
                            | (input[sample_idx] as i32);
                        let val = if val & I24_SIGN_BIT != 0 {
                            val | I24_SIGN_EXTENSION_MASK
                        } else {
                            val
                        };
                        val as f32 / I24_DIVISOR
                    }
                    BIT_DEPTH_32 => {
                        if is_float_format {
                            let bytes = [
                                input[sample_idx],
                                input[sample_idx + 1],
                                input[sample_idx + 2],
                                input[sample_idx + 3],
                            ];
                            f32::from_le_bytes(bytes)
                        } else {
                            let val = i32::from_le_bytes([
                                input[sample_idx],
                                input[sample_idx + 1],
                                input[sample_idx + 2],
                                input[sample_idx + 3],
                            ]);
                            val as f32 / I32_DIVISOR
                        }
                    }
                    _ => 0.0, // Should never reach here due to earlier check
                };

                channel_data.push(val);
            }

            channel_data
        })
        .collect();

    Ok(output)
}

fn encode_samples<W: Write>(out: &mut W, buffer: &AudioBuffer, bits_per_sample: u16) -> R<()> {
    // Ensure channel count doesn't exceed available data channels
    let available_channels = buffer.data.len();
    let channels = std::cmp::min(buffer.channels as usize, available_channels);

    // Ensure consistent channel count between metadata and actual data
    let frames = buffer.data[0].len();

    for i in 0..frames {
        for ch in 0..channels {
            let sample = buffer.data[ch][i];
            match bits_per_sample {
                BIT_DEPTH_8 => {
                    let val = ((sample * U8_SCALE + U8_OFFSET).clamp(0.0, 255.0)) as u8;
                    out.write_u8(val)?;
                }
                BIT_DEPTH_16 => {
                    let val = (sample.clamp(-1.0, 1.0) * I16_MAX_F) as i16;
                    out.write_i16::<LittleEndian>(val)?;
                }
                BIT_DEPTH_24 => {
                    let val = (sample.clamp(-1.0, 1.0) * I24_MAX_F) as i32;
                    let bytes = [
                        (val & BYTE_MASK) as u8,
                        ((val >> 8) & BYTE_MASK) as u8,
                        ((val >> 16) & BYTE_MASK) as u8,
                    ];
                    out.write_all(&bytes)?;
                }
                BIT_DEPTH_32 => {
                    if buffer.format == SampleFormat::F32 {
                        out.write_f32::<LittleEndian>(sample)?;
                    } else {
                        let val = (sample.clamp(-1.0, 1.0) * I32_MAX_F) as i32;
                        out.write_i32::<LittleEndian>(val)?;
                    }
                }
                _ => return Err(anyhow!("Unsupported bit depth")),
            }
        }
    }

    Ok(())
}

fn write_chunk<W: Write>(writer: &mut W, id: &[u8], data: &[u8]) -> R<()> {
    writer.write_all(id)?;
    writer.write_u32::<LittleEndian>(data.len() as u32)?;
    writer.write_all(data)?;
    if data.len() % 2 == 1 {
        writer.write_all(&[0])?; // padding
    }
    Ok(())
}

// Helper function to extract comment from ID3 data
fn extract_id3_comment(id3_data: &[u8]) -> String {
    // Check if it's ID3v2
    if id3_data.len() >= 10 && &id3_data[0..3] == b"ID3" {
        // ID3v2 format - skip header and look for frames
        let mut offset = 10; // Skip ID3v2 header

        while offset + 10 < id3_data.len() {
            // ID3v2 frame header: frame_id (4 bytes) + size (4 bytes) + flags (2 bytes)
            let frame_id = &id3_data[offset..offset + 4];

            // Read frame size (big-endian for ID3v2.3/2.4)
            let frame_size = ((id3_data[offset + 4] as u32) << 24)
                | ((id3_data[offset + 5] as u32) << 16)
                | ((id3_data[offset + 6] as u32) << 8)
                | (id3_data[offset + 7] as u32);

            if frame_size == 0 || offset + 10 + frame_size as usize > id3_data.len() {
                break;
            }

            // Check for comment frames (COMM)
            if frame_id == b"COMM" {
                let frame_data = &id3_data[offset + 10..offset + 10 + frame_size as usize];

                if frame_data.len() > 4 {
                    // Skip encoding byte (1), language (3), and short description
                    let mut text_start = 4;

                    // Find the end of the short description (null terminated)
                    while text_start < frame_data.len() && frame_data[text_start] != 0 {
                        text_start += 1;
                    }
                    text_start += 1; // Skip the null terminator

                    if text_start < frame_data.len() {
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
    } else {
        // Try to parse as raw comment data
        let text = String::from_utf8_lossy(id3_data)
            .trim_end_matches('\0')
            .trim()
            .to_string();

        if !text.is_empty() {
            return text;
        }
    }

    String::new()
}
