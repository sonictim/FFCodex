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

// Bit depth constants
const BIT_DEPTH_8: u16 = 8;
const BIT_DEPTH_16: u16 = 16;
const BIT_DEPTH_24: u16 = 24;
const BIT_DEPTH_32: u16 = 32;

// Sample conversion constants
const U8_SCALE: f32 = 127.0;
const U8_OFFSET: f32 = 128.0;
const I16_MAX_F: f32 = 32767.0;
const I16_DIVISOR: f32 = 32768.0;
const I16_DIVISOR_RECIP: f32 = 1.0 / 32768.0;
const I24_MAX_F: f32 = 8388607.0;
const I24_DIVISOR: f32 = 8388608.0;
const I24_DIVISOR_RECIP: f32 = 1.0 / 8388608.0;
const I32_MAX_F: f32 = 2147483647.0;
const I32_DIVISOR: f32 = 2147483648.0;
const I32_DIVISOR_RECIP: f32 = 1.0 / 2147483648.0;
const I24_SIGN_BIT: i32 = 0x800000;
const I24_SIGN_EXTENSION_MASK: i32 = -16777216; // 0xFF000000 as i32
const BYTE_MASK: i32 = 0xFF;

pub struct WavCodec;

#[derive(Debug, Clone)]
struct AudioInfo {
    format_tag: u16,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
}

impl Codec for WavCodec {
    fn as_str(&self) -> &'static str {
        "WAV"
    }

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

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata> {
        let mut metadata = Metadata::new();
        let mut cursor = Cursor::new(input);

        // Validate WAV header
        self.validate_file_format(input)?;

        // Skip RIFF header (12 bytes)
        cursor.set_position(12);

        // Parse chunks
        while cursor.position() < input.len() as u64 {
            // Read chunk header
            let chunk_id = match cursor.read_u32::<LittleEndian>() {
                Ok(id) => id,
                Err(_) => break, // End of file
            };

            let chunk_size = match cursor.read_u32::<LittleEndian>() {
                Ok(size) => size as usize,
                Err(_) => break,
            };

            let chunk_start = cursor.position() as usize;

            // Ensure we don't read past the end of the input
            if chunk_start + chunk_size > input.len() {
                break;
            }

            let chunk_data = &input[chunk_start..chunk_start + chunk_size];

            // Parse different chunk types
            match &chunk_id.to_le_bytes() {
                b"fmt " => {
                    // Parse fmt chunk to extract audio format information
                    if chunk_data.len() >= 16 {
                        let mut fmt_cursor = Cursor::new(chunk_data);
                        metadata.format_tag = fmt_cursor.read_u16::<LittleEndian>()?;
                        metadata.channels = fmt_cursor.read_u16::<LittleEndian>()?;
                        metadata.sample_rate = fmt_cursor.read_u32::<LittleEndian>()?;
                        fmt_cursor.read_u32::<LittleEndian>()?; // byte_rate - skip
                        fmt_cursor.read_u16::<LittleEndian>()?; // block_align - skip
                        metadata.bit_depth = fmt_cursor.read_u16::<LittleEndian>()?;
                    }
                }
                b"bext" => {
                    metadata.parse_bext(chunk_data)?;
                }
                b"iXML" => {
                    let xml_str = String::from_utf8_lossy(chunk_data);
                    metadata.parse_ixml(&xml_str)?;
                }
                b"ID3 " | b"id3 " => {
                    metadata.parse_id3(chunk_data)?;
                }
                b"SMED" | b"SMRD" | b"SMPL" | b"APIC" => {
                    // Skip binary metadata chunks - these contain non-text data
                    // SMED = Soundminer metadata (binary)
                    // SMRD = Soundminer (binary)
                    // SMPL = Sample data (binary)
                    // APIC = Album picture (binary)
                }
                _ => {
                    // Skip unknown chunks - only process known text-based chunks above
                    // This prevents binary data from being interpreted as text
                }
            }

            // Move to next chunk (pad to even byte boundary)
            cursor.set_position(chunk_start as u64 + chunk_size as u64);
            if chunk_size % 2 == 1 {
                cursor.set_position(cursor.position() + 1);
            }
        }

        Ok(metadata)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Metadata) -> R<()> {
        // Read the existing file
        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };

        // Create new metadata from the hashmap
        let new_data = self.embed_metadata_from_hashmap(&mapped_file, metadata)?;

        // Write the data back to the file
        let _data_len = new_data.len();
        std::fs::write(file_path, new_data)?;
        Ok(())
    }
}

impl WavCodec {
    fn embed_metadata_from_hashmap(&self, input: &[u8], metadata: &Metadata) -> R<Vec<u8>> {
        let mut output = Cursor::new(Vec::new());

        // Extract audio data using the metadata's audio format information
        let data_chunk = self.extract_audio_data_only(input)?;

        // Create AudioInfo from metadata's format information
        let audio_info = AudioInfo {
            format_tag: metadata.format_tag,
            channels: metadata.channels,
            sample_rate: metadata.sample_rate,
            byte_rate: metadata.sample_rate
                * metadata.channels as u32
                * (metadata.bit_depth / 8) as u32,
            block_align: metadata.channels * (metadata.bit_depth / 8),
            bits_per_sample: metadata.bit_depth,
        };

        // Start building the clean WAV file
        // Write RIFF header (will update size later)
        output.write_all(b"RIFF")?;
        output.write_u32::<LittleEndian>(0)?; // Placeholder for file size
        output.write_all(b"WAVE")?;

        // Write fmt chunk - build from audio info
        self.write_fmt_chunk(&mut output, &audio_info)?;

        // Write BEXT chunk from metadata
        self.write_bext_chunk(&mut output, metadata)?;

        // Write iXML chunk from metadata
        self.write_ixml_chunk(&mut output, metadata)?;

        // Write image chunks if any exist
        self.write_image_chunks(&mut output, metadata)?;

        // Write data chunk
        output.write_all(b"data")?;
        output.write_u32::<LittleEndian>(data_chunk.len() as u32)?;
        output.write_all(&data_chunk)?;
        if data_chunk.len() % 2 == 1 {
            output.write_all(&[0])?; // Padding
        }

        // Update RIFF file size
        let total_size = output.position() as u32 - 8; // Exclude RIFF header itself
        output.seek(SeekFrom::Start(4))?;
        output.write_u32::<LittleEndian>(total_size)?;

        Ok(output.into_inner())
    }

    fn extract_audio_data(&self, input: &[u8]) -> R<(AudioInfo, Vec<u8>)> {
        let mut cursor = Cursor::new(input);

        // Skip RIFF header
        cursor.seek(SeekFrom::Start(12))?;

        let mut audio_info = None;
        let mut data_chunk = None;

        while cursor.position() < input.len() as u64 {
            let mut chunk_id = [0u8; 4];
            if cursor.read(&mut chunk_id)? < 4 {
                break;
            }

            let chunk_size = cursor.read_u32::<LittleEndian>()?;
            let chunk_start = cursor.position();

            match &chunk_id {
                b"fmt " => {
                    // Parse fmt chunk to get audio format info
                    let format_tag = cursor.read_u16::<LittleEndian>()?;
                    let channels = cursor.read_u16::<LittleEndian>()?;
                    let sample_rate = cursor.read_u32::<LittleEndian>()?;
                    let byte_rate = cursor.read_u32::<LittleEndian>()?;
                    let block_align = cursor.read_u16::<LittleEndian>()?;
                    let bits_per_sample = cursor.read_u16::<LittleEndian>()?;

                    audio_info = Some(AudioInfo {
                        format_tag,
                        channels,
                        sample_rate,
                        byte_rate,
                        block_align,
                        bits_per_sample,
                    });
                }
                b"data" => {
                    let mut chunk_data = vec![0u8; chunk_size as usize];
                    cursor.read_exact(&mut chunk_data)?;
                    data_chunk = Some(chunk_data);
                }
                _ => {
                    // Skip all other chunks
                }
            }

            // Move to next chunk (pad to even byte boundary)
            cursor.set_position(chunk_start + chunk_size as u64);
            if chunk_size % 2 == 1 {
                cursor.set_position(cursor.position() + 1);
            }
        }

        let audio_info = audio_info.ok_or_else(|| anyhow!("No fmt chunk found"))?;
        let data_chunk = data_chunk.ok_or_else(|| anyhow!("No data chunk found"))?;

        Ok((audio_info, data_chunk))
    }

    fn extract_audio_data_only(&self, input: &[u8]) -> R<Vec<u8>> {
        let mut cursor = Cursor::new(input);

        // Skip RIFF header
        cursor.seek(SeekFrom::Start(12))?;

        let mut data_chunk = None;

        while cursor.position() < input.len() as u64 {
            let mut chunk_id = [0u8; 4];
            if cursor.read(&mut chunk_id)? < 4 {
                break;
            }

            let chunk_size = cursor.read_u32::<LittleEndian>()?;
            let chunk_start = cursor.position();

            // Ensure we don't read past the end of the input
            if chunk_start as usize + chunk_size as usize > input.len() {
                break;
            }

            match &chunk_id {
                b"data" => {
                    let mut chunk_data = vec![0u8; chunk_size as usize];
                    cursor.read_exact(&mut chunk_data)?;
                    data_chunk = Some(chunk_data);
                    break; // Found data chunk, no need to continue
                }
                _ => {
                    // Skip all other chunks - don't read the data, just advance cursor
                    cursor.set_position(chunk_start + chunk_size as u64);
                    if chunk_size % 2 == 1 {
                        cursor.set_position(cursor.position() + 1);
                    }
                }
            }
        }

        let data_chunk = data_chunk.ok_or_else(|| anyhow!("No data chunk found"))?;

        Ok(data_chunk)
    }

    fn write_fmt_chunk(&self, output: &mut Cursor<Vec<u8>>, audio_info: &AudioInfo) -> R<()> {
        output.write_all(b"fmt ")?;
        output.write_u32::<LittleEndian>(16)?; // Standard PCM fmt chunk size

        // Always write a clean, standard PCM format chunk
        let format_tag = if audio_info.format_tag == FORMAT_IEEE_FLOAT {
            FORMAT_IEEE_FLOAT
        } else {
            FORMAT_PCM
        };

        // Recalculate byte_rate and block_align to ensure they're correct
        let bytes_per_sample = audio_info.bits_per_sample / 8;
        let block_align = audio_info.channels * bytes_per_sample;
        let byte_rate = audio_info.sample_rate * block_align as u32;

        output.write_u16::<LittleEndian>(format_tag)?;
        output.write_u16::<LittleEndian>(audio_info.channels)?;
        output.write_u32::<LittleEndian>(audio_info.sample_rate)?;
        output.write_u32::<LittleEndian>(byte_rate)?;
        output.write_u16::<LittleEndian>(block_align)?;
        output.write_u16::<LittleEndian>(audio_info.bits_per_sample)?;

        Ok(())
    }

    fn write_bext_chunk(&self, output: &mut Cursor<Vec<u8>>, metadata: &Metadata) -> R<()> {
        let mut bext_data = vec![0u8; 602]; // BWF spec minimum size

        // Description (256 bytes) - try multiple field names
        if let Some(description) = metadata
            .get_field("DESCRIPTION")
            .or_else(|| metadata.get_field("BEXT_BWF_DESCRIPTION"))
            .or_else(|| metadata.get_field("Description"))
        {
            let bytes = description.as_bytes();
            let len = std::cmp::min(bytes.len(), 255);
            bext_data[..len].copy_from_slice(&bytes[..len]);
        }

        // Originator (32 bytes) - try multiple field names
        if let Some(originator) = metadata
            .get_field("USER_DESIGNER")
            .or_else(|| metadata.get_field("BEXT_BWF_ORIGINATOR"))
            .or_else(|| metadata.get_field("Originator"))
        {
            let bytes = originator.as_bytes();
            let len = std::cmp::min(bytes.len(), 31);
            bext_data[256..256 + len].copy_from_slice(&bytes[..len]);
        }

        // OriginatorReference (32 bytes) - try multiple field names
        if let Some(orig_ref) = metadata
            .get_field("BEXT_BWF_ORIGINATOR_REFERENCE")
            .or_else(|| metadata.get_field("OriginatorReference"))
        {
            let bytes = orig_ref.as_bytes();
            let len = std::cmp::min(bytes.len(), 31);
            bext_data[288..288 + len].copy_from_slice(&bytes[..len]);
        }

        // OriginationDate (10 bytes)
        if let Some(date) = metadata.get_field("OriginationDate") {
            let bytes = date.as_bytes();
            let len = std::cmp::min(bytes.len(), 10);
            bext_data[320..320 + len].copy_from_slice(&bytes[..len]);
        }

        // OriginationTime (8 bytes)
        if let Some(time) = metadata.get_field("OriginationTime") {
            let bytes = time.as_bytes();
            let len = std::cmp::min(bytes.len(), 8);
            bext_data[330..330 + len].copy_from_slice(&bytes[..len]);
        }

        // TimeReference (8 bytes) - try multiple field names
        if let Some(time_ref) = metadata
            .get_field("TimeReference")
            .or_else(|| metadata.get_field("BEXT_BWF_TIME_REFERENCE_LOW"))
        {
            if let Ok(time_ref_val) = time_ref.parse::<u64>() {
                (&mut bext_data[338..346]).write_u64::<LittleEndian>(time_ref_val)?;
            }
        }

        // Write bext chunk
        output.write_all(b"bext")?;
        output.write_u32::<LittleEndian>(bext_data.len() as u32)?;
        output.write_all(&bext_data)?;
        if bext_data.len() % 2 == 1 {
            output.write_all(&[0])?; // Padding
        }

        Ok(())
    }

    fn write_ixml_chunk(&self, output: &mut Cursor<Vec<u8>>, metadata: &Metadata) -> R<()> {
        let ixml_content = self.create_ixml(metadata)?;
        if !ixml_content.trim().is_empty() {
            let ixml_bytes = ixml_content.as_bytes();
            output.write_all(b"iXML")?;
            output.write_u32::<LittleEndian>(ixml_bytes.len() as u32)?;
            output.write_all(ixml_bytes)?;
            if ixml_bytes.len() % 2 == 1 {
                output.write_all(&[0])?; // Padding
            }
        }
        Ok(())
    }

    fn write_image_chunks(&self, output: &mut Cursor<Vec<u8>>, metadata: &Metadata) -> R<()> {
        // Write image chunks if any exist
        for image in metadata.get_images() {
            output.write_all(b"APIC")?; // Or use appropriate chunk ID
            output.write_u32::<LittleEndian>(image.data.len() as u32)?;
            output.write_all(&image.data)?;
            if image.data.len() % 2 == 1 {
                output.write_all(&[0])?; // Padding
            }
        }
        Ok(())
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

    // Use parallel processing only for files with many channels or large frame counts
    let use_parallel = channels > 4 && frame_count > 10_000;

    let channel_processor = |ch: usize| {
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

            let val = match bits_per_sample {
                8 => {
                    let sample = input[sample_idx] as f32;
                    (sample - U8_OFFSET) / U8_SCALE
                }
                16 => {
                    let sample =
                        i16::from_le_bytes([input[sample_idx], input[sample_idx + 1]]) as f32;
                    sample * I16_DIVISOR_RECIP
                }
                24 => {
                    let mut sample = i32::from_le_bytes([
                        input[sample_idx],
                        input[sample_idx + 1],
                        input[sample_idx + 2],
                        0,
                    ]);
                    if sample & I24_SIGN_BIT != 0 {
                        sample |= I24_SIGN_EXTENSION_MASK;
                    }
                    sample as f32 * I24_DIVISOR_RECIP
                }
                32 => {
                    let sample = i32::from_le_bytes([
                        input[sample_idx],
                        input[sample_idx + 1],
                        input[sample_idx + 2],
                        input[sample_idx + 3],
                    ]) as f32;
                    sample * I32_DIVISOR_RECIP
                }
                _ => 0.0,
            };

            channel_data.push(val);
        }
        channel_data
    };

    let output: Vec<Vec<f32>> = if use_parallel {
        (0..channels as usize)
            .into_par_iter()
            .map(channel_processor)
            .collect()
    } else {
        (0..channels as usize).map(channel_processor).collect()
    };

    Ok(output)
}

// ...existing code...

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
