use crate::prelude::*;

// Format tags
// const FORMAT_PCM: u16 = 1;
// const FORMAT_IEEE_FLOAT: u16 = 3;

// Chunk Identifiers
const FORM_CHUNK_ID: &[u8; 4] = b"FORM";
const AIFF_FORMAT_ID: &[u8; 4] = b"AIFF";
const FMT_CHUNK_ID: &[u8; 4] = b"COMM";
const DATA_CHUNK_ID: &[u8; 4] = b"SSND";

// AIFF Metadata Chunk Identifiers
const ANNO_CHUNK_ID: &[u8; 4] = b"ANNO";
const COMT_CHUNK_ID: &[u8; 4] = b"COMT";
const NAME_CHUNK_ID: &[u8; 4] = b"NAME";
const AUTH_CHUNK_ID: &[u8; 4] = b"AUTH";
const COPYRIGHT_CHUNK_ID: &[u8; 4] = b"(c) ";
const APPL_CHUNK_ID: &[u8; 4] = b"APPL";
const ID3_CHUNK_ID: &[u8; 4] = b"ID3 ";

// Chunk Structures
const HEADER_SIZE: usize = 12; // FORM + size + AIFF
const MIN_VALID_FILE_SIZE: usize = 12;

pub struct AifCodec;

impl Codec for AifCodec {
    fn file_extension(&self) -> &'static str {
        "aif"
    }

    fn validate_file_format(&self, data: &[u8]) -> R<()> {
        if data.len() < MIN_VALID_FILE_SIZE {
            return Err(anyhow!("File too small to be a valid AIFF"));
        }

        let mut cursor = Cursor::new(data);

        // Read FORM header
        let mut form = [0u8; 4];
        cursor.read_exact(&mut form)?;
        if &form != FORM_CHUNK_ID {
            return Err(anyhow!("Not a FORM file"));
        }

        cursor.read_u32::<BigEndian>()?; // File size
        let mut aiff = [0u8; 4];
        cursor.read_exact(&mut aiff)?;
        if &aiff != AIFF_FORMAT_ID {
            return Err(anyhow!("Not an AIFF file"));
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

        let mut cursor = Cursor::new(&mapped_file[..]);
        cursor.set_position(HEADER_SIZE as u64);

        let mut sample_rate = 0u16;
        let mut channels = 0u16;
        let mut bits_per_sample = 0u16;
        let mut total_frames = 0u32;

        // Find the COMM chunk to extract audio format information
        while (cursor.position() as usize) < mapped_file.len() {
            if cursor.position() + 8 > mapped_file.len() as u64 {
                break;
            }

            let mut chunk_id = [0u8; 4];
            cursor.read_exact(&mut chunk_id)?;
            let chunk_size = cursor.read_u32::<BigEndian>()? as usize;

            if cursor.position() as usize + chunk_size > mapped_file.len() {
                break;
            }

            if &chunk_id == FMT_CHUNK_ID {
                // Found COMM chunk - extract format information
                channels = cursor.read_u16::<BigEndian>()?;
                total_frames = cursor.read_u32::<BigEndian>()?;
                bits_per_sample = cursor.read_u16::<BigEndian>()?;

                // Read the 80-bit IEEE extended sample rate
                sample_rate = read_ieee_extended(&mut cursor)? as u16;
                break;
            } else {
                // Skip this chunk
                cursor.set_position(cursor.position() + chunk_size as u64);
                if chunk_size % 2 == 1 {
                    cursor.set_position(cursor.position() + 1); // Skip padding
                }
            }
        }

        if sample_rate == 0 || channels == 0 {
            return Err(anyhow!("Could not find valid COMM chunk in AIFF file"));
        }

        // Calculate duration
        let duration_seconds = if sample_rate > 0 {
            total_frames as f64 / sample_rate as f64
        } else {
            0.0
        };

        let duration = if duration_seconds >= 3600.0 {
            format!("{:.0}:{:02.0}:{:02.0}", 
                duration_seconds / 3600.0, 
                (duration_seconds % 3600.0) / 60.0, 
                duration_seconds % 60.0)
        } else {
            format!("{:.0}:{:02.0}", 
                duration_seconds / 60.0, 
                duration_seconds % 60.0)
        };

        Ok(FileInfo {
            path: file_path.to_string(),
            size: file_size,
            sample_rate,
            channels,
            bit_depth: bits_per_sample,
            duration,
        })
    }

    fn encode(&self, buffer: &Option<AudioBuffer>) -> R<Vec<u8>> {
        let mut output = Cursor::new(Vec::new());

        let Some(buffer) = buffer else {
            return Err(anyhow!("Cannot encode None AudioBuffer"));
        };

        // Validate input buffer
        if buffer.data.is_empty() {
            return Err(anyhow!("Cannot encode empty audio buffer"));
        }

        // Ensure all channels have the same length
        let frame_count = buffer.data[0].len();
        for (i, channel) in buffer.data.iter().enumerate() {
            if channel.len() != frame_count {
                return Err(anyhow!(
                    "Channel {} has {} samples, expected {}",
                    i,
                    channel.len(),
                    frame_count
                ));
            }
        }

        // Write FORM header
        output.write_all(FORM_CHUNK_ID)?;
        output.write_u32::<BigEndian>(0)?; // Placeholder for file size
        output.write_all(AIFF_FORMAT_ID)?;

        // Write COMM chunk
        output.write_all(FMT_CHUNK_ID)?;
        output.write_u32::<BigEndian>(18)?; // COMM chunk size
        output.write_u16::<BigEndian>(buffer.channels)?;

        // Write number of sample frames
        let num_frames = frame_count as u32;
        output.write_u32::<BigEndian>(num_frames)?;

        // Get bit depth from format
        let bits_per_sample = match buffer.format {
            SampleFormat::F32 => 32,
            SampleFormat::I16 => 16,
            SampleFormat::I24 => 24,
            SampleFormat::I32 => 32,
            SampleFormat::U8 => 8,
        };
        output.write_u16::<BigEndian>(bits_per_sample)?;

        // Write extended 80-bit IEEE 754 format for sample rate
        // This is required by AIFF spec
        write_ieee_extended_simple(&mut output, buffer.sample_rate as f64)?;

        // Write SSND chunk header
        output.write_all(DATA_CHUNK_ID)?;
        let ssnd_chunk_size_pos = output.position();
        output.write_u32::<BigEndian>(0)?; // Placeholder for chunk size
        output.write_u32::<BigEndian>(0)?; // Offset
        output.write_u32::<BigEndian>(0)?; // Block size

        let start_data = output.position();

        let mut interleaved_bytes = Vec::new();
        encode_samples(&mut interleaved_bytes, buffer, bits_per_sample)?;
        output.write_all(&interleaved_bytes)?;

        let end_data = output.position();
        let data_size = (end_data - start_data) as u32;
        let ssnd_chunk_size = data_size + 8; // Add 8 bytes for offset and block size

        // Fill in SSND chunk size
        let mut out = output.into_inner();
        (&mut out[ssnd_chunk_size_pos as usize..(ssnd_chunk_size_pos + 4) as usize])
            .write_u32::<BigEndian>(ssnd_chunk_size)?;

        // Fill in FORM file size
        let form_size = out.len() as u32 - 8;
        (&mut out[4..8]).write_u32::<BigEndian>(form_size)?;

        Ok(out)
    }

    fn decode(&self, input: &[u8]) -> R<AudioBuffer> {
        self.validate_file_format(input)?;

        let mut cursor = Cursor::new(input);
        cursor.set_position(HEADER_SIZE as u64);

        let mut fmt_chunk_found = false;
        let mut data_chunk_found = false;
        let mut sample_format = SampleFormat::I16;
        let mut channels = 0;
        let mut sample_rate = 0;
        let mut bits_per_sample = 0;
        let mut audio_data = vec![];

        while (cursor.position() as usize) < input.len() {
            // Check if we have enough bytes for chunk header
            if cursor.position() + 8 > input.len() as u64 {
                break;
            }

            let mut chunk_id = [0u8; 4];
            cursor.read_exact(&mut chunk_id)?;
            let chunk_size = cursor.read_u32::<BigEndian>()? as usize;

            // Check if chunk size would exceed input bounds
            if cursor.position() as usize + chunk_size > input.len() {
                break;
            }

            match &chunk_id {
                FMT_CHUNK_ID => {
                    fmt_chunk_found = true;
                    channels = cursor.read_u16::<BigEndian>()?;
                    let _frames = cursor.read_u32::<BigEndian>()?; // Total frames - we read but don't use
                    bits_per_sample = cursor.read_u16::<BigEndian>()?;

                    // Use read_ieee_extended to get the sample rate (80-bit extended precision)
                    sample_rate = read_ieee_extended(&mut cursor)? as u32;

                    // For AIFF, 32-bit samples are typically integers unless specifically IEEE float
                    // AIFF format doesn't have a format tag like WAV, so we assume integer for most cases
                    sample_format = match bits_per_sample {
                        8 => SampleFormat::U8,
                        16 => SampleFormat::I16,
                        24 => SampleFormat::I24,
                        32 => SampleFormat::I32, // Default to integer; could be float but rare in AIFF
                        _ => {
                            return Err(anyhow!("Unsupported bit depth: {}", bits_per_sample));
                        }
                    };
                }
                DATA_CHUNK_ID => {
                    data_chunk_found = true;
                    cursor.read_u32::<BigEndian>()?; // Offset
                    cursor.read_u32::<BigEndian>()?; // Block size

                    if chunk_size < 8 {
                        return Err(anyhow!("Invalid SSND chunk size"));
                    }

                    let audio_data_size = chunk_size - 8;
                    let mut raw_data = vec![0u8; audio_data_size];
                    cursor.read_exact(&mut raw_data)?;

                    audio_data = decode_samples(
                        &raw_data,
                        channels,
                        bits_per_sample,
                        sample_format == SampleFormat::F32,
                    )?;
                }

                _ => {
                    // Skip unknown chunks safely
                    cursor.set_position(cursor.position() + chunk_size as u64);
                }
            }
        }

        if !fmt_chunk_found || !data_chunk_found {
            return Err(anyhow!("Missing 'COMM' or 'SSND' chunk"));
        }

        Ok(AudioBuffer {
            sample_rate,
            channels,
            format: sample_format,
            data: audio_data,
        })
    }

    fn extract_metadata_from_file(&self, file_path: &str) -> R<Metadata> {
        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };
        let chunks = self.extract_metadata_chunks(&mapped_file)?;
        Ok(Metadata::Wav(chunks)) // Use WAV metadata type since AIFF uses similar chunk structure
    }

    fn extract_metadata_chunks(&self, input: &[u8]) -> R<Vec<MetadataChunk>> {
        let mut cursor = Cursor::new(input);

        // Validate AIFF header
        let mut header = [0u8; 12];
        cursor.read_exact(&mut header)?;

        if &header[0..4] != FORM_CHUNK_ID || &header[8..12] != AIFF_FORMAT_ID {
            return Err(anyhow!("Not an AIFF file"));
        }

        let mut chunks = Vec::new();
        while cursor.position() < input.len() as u64 {
            let mut id = [0u8; 4];
            if cursor.read(&mut id)? < 4 {
                break;
            }

            let size = cursor.read_u32::<BigEndian>()?;

            // Skip audio format and data chunks - they're not metadata
            if &id == FMT_CHUNK_ID || &id == DATA_CHUNK_ID {
                cursor.seek(SeekFrom::Current(size as i64 + (size % 2) as i64))?;
                continue;
            }

            let mut data = vec![0u8; size as usize];
            cursor.read_exact(&mut data)?;

            let chunk = match &id {
                ANNO_CHUNK_ID => {
                    // AIFF annotation chunk - convert to text tag
                    let text = String::from_utf8_lossy(&data)
                        .trim_end_matches('\0')
                        .to_string();
                    chunks.push(MetadataChunk::TextTag {
                        key: "ANNOTATION".to_string(),
                        value: text.clone(),
                    });
                    MetadataChunk::Unknown {
                        id: "ANNO".to_string(),
                        data,
                    }
                }
                COMT_CHUNK_ID => {
                    // AIFF comment chunk - convert to iXML style format
                    if data.len() >= 2 {
                        let num_comments = u16::from_be_bytes([data[0], data[1]]) as usize;
                        let mut comments = String::new();
                        let mut offset = 2;

                        for i in 0..num_comments {
                            if offset + 8 <= data.len() {
                                // Each comment has: timestamp (4 bytes) + marker_id (2 bytes) + count (2 bytes) + text
                                let _timestamp = u32::from_be_bytes([
                                    data[offset],
                                    data[offset + 1],
                                    data[offset + 2],
                                    data[offset + 3],
                                ]);
                                let _marker_id =
                                    u16::from_be_bytes([data[offset + 4], data[offset + 5]]);
                                let text_len =
                                    u16::from_be_bytes([data[offset + 6], data[offset + 7]])
                                        as usize;
                                offset += 8;

                                if offset + text_len <= data.len() {
                                    let comment_text =
                                        String::from_utf8_lossy(&data[offset..offset + text_len])
                                            .to_string();
                                    comments.push_str(&format!(
                                        "COMMENT{}={}\n",
                                        i + 1,
                                        comment_text
                                    ));

                                    chunks.push(MetadataChunk::TextTag {
                                        key: format!("COMMENT{}", i + 1),
                                        value: comment_text,
                                    });

                                    offset += text_len;
                                    // Handle padding
                                    if text_len % 2 == 1 {
                                        offset += 1;
                                    }
                                }
                            }
                        }

                        if !comments.is_empty() {
                            chunks.push(MetadataChunk::IXml(comments));
                        }
                    }

                    MetadataChunk::Unknown {
                        id: "COMT".to_string(),
                        data,
                    }
                }
                NAME_CHUNK_ID => {
                    let text = String::from_utf8_lossy(&data)
                        .trim_end_matches('\0')
                        .to_string();
                    chunks.push(MetadataChunk::TextTag {
                        key: "TITLE".to_string(),
                        value: text.clone(),
                    });
                    MetadataChunk::Unknown {
                        id: "NAME".to_string(),
                        data,
                    }
                }
                AUTH_CHUNK_ID => {
                    let text = String::from_utf8_lossy(&data)
                        .trim_end_matches('\0')
                        .to_string();
                    chunks.push(MetadataChunk::TextTag {
                        key: "ARTIST".to_string(),
                        value: text.clone(),
                    });
                    MetadataChunk::Unknown {
                        id: "AUTH".to_string(),
                        data,
                    }
                }
                COPYRIGHT_CHUNK_ID => {
                    let text = String::from_utf8_lossy(&data)
                        .trim_end_matches('\0')
                        .to_string();
                    chunks.push(MetadataChunk::TextTag {
                        key: "COPYRIGHT".to_string(),
                        value: text.clone(),
                    });
                    MetadataChunk::Unknown {
                        id: "(c) ".to_string(),
                        data,
                    }
                }
                APPL_CHUNK_ID => {
                    // Application specific data - keep as unknown for now
                    MetadataChunk::Unknown {
                        id: "APPL".to_string(),
                        data,
                    }
                }
                ID3_CHUNK_ID => MetadataChunk::ID3(data),
                _ => MetadataChunk::Unknown {
                    id: String::from_utf8_lossy(&id).to_string(),
                    data,
                },
            };

            chunks.push(chunk);

            // AIFF chunks are padded to even byte boundaries
            if size % 2 == 1 {
                cursor.seek(SeekFrom::Current(1))?;
            }
        }

        Ok(chunks)
    }

    fn embed_metadata_to_file(&self, file_path: &str, metadata: &Option<Metadata>) -> R<()> {
        let Some(metadata) = metadata else {
            return Err(anyhow!("No metadata to embed"));
        };
        let chunks = match metadata {
            Metadata::Wav(chunks) => chunks, // AIFF uses same chunk structure as WAV
            _ => return Err(anyhow!("Unsupported metadata format for AIFF")),
        };

        let file = std::fs::File::open(file_path)?;
        let mapped_file = unsafe { MmapOptions::new().map(&file)? };
        let new_data = self.embed_metadata_chunks(&mapped_file, chunks)?;
        std::fs::write(file_path, new_data)?;
        Ok(())
    }

    fn embed_metadata_chunks(&self, input: &[u8], chunks: &[MetadataChunk]) -> R<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut output = Cursor::new(Vec::new());

        // Copy the FORM/AIFF header
        let mut form_header = [0u8; 12];
        cursor.read_exact(&mut form_header)?;
        output.write_all(&form_header)?;

        // Group metadata by type for better organization
        let mut text_tags = Vec::new();
        let mut ixml_chunks = Vec::new();
        let mut id3_chunks = Vec::new();
        let mut other_chunks = Vec::new();

        // Process and organize metadata chunks
        for chunk in chunks {
            match chunk {
                MetadataChunk::IXml(xml) => ixml_chunks.push(xml.clone()),
                MetadataChunk::TextTag { key, value } => {
                    text_tags.push((key.clone(), value.clone()))
                }
                MetadataChunk::ID3(data) => id3_chunks.push(data.clone()),
                MetadataChunk::Unknown { id, data } => {
                    other_chunks.push((id.clone(), data.clone()))
                }
                _ => {
                    // Convert other types to unknown chunks for AIFF
                    other_chunks.push((chunk.id(), chunk.data().to_vec()));
                }
            }
        }

        // Read through input file and copy non-metadata chunks
        let mut comm_chunk_found = false;
        while cursor.position() < input.len() as u64 {
            let mut id = [0u8; 4];
            if cursor.read(&mut id)? < 4 {
                break;
            }

            let size = cursor.read_u32::<BigEndian>()?;
            let mut data = vec![0u8; size as usize];
            cursor.read_exact(&mut data)?;

            let id_str = String::from_utf8_lossy(&id).to_string();

            // Handle COMM chunk specially to preserve it
            if &id == FMT_CHUNK_ID {
                comm_chunk_found = true;
                aiff_write_chunk(&mut output, &id, &data)?;

                if size % 2 == 1 {
                    cursor.seek(SeekFrom::Current(1))?;
                }
                continue;
            }

            // Skip known metadata chunks since we'll replace them
            if matches!(
                id_str.as_str(),
                "ANNO" | "COMT" | "NAME" | "AUTH" | "(c) " | "APPL" | "ID3 "
            ) {
                if size % 2 == 1 {
                    cursor.seek(SeekFrom::Current(1))?;
                }
                continue;
            }

            // Write other chunks directly to output (like SSND data chunk)
            aiff_write_chunk(&mut output, &id, &data)?;

            if size % 2 == 1 {
                cursor.seek(SeekFrom::Current(1))?;
            }
        }

        if !comm_chunk_found {
            return Err(anyhow!("AIFF file missing COMM chunk"));
        }

        // Write metadata chunks

        // Convert text tags to AIFF chunks
        for (key, value) in &text_tags {
            let chunk_id = match key.as_str() {
                "TITLE" => NAME_CHUNK_ID,
                "ARTIST" => AUTH_CHUNK_ID,
                "COPYRIGHT" => COPYRIGHT_CHUNK_ID,
                "ANNOTATION" => ANNO_CHUNK_ID,
                _ => ANNO_CHUNK_ID, // Default to annotation
            };
            aiff_write_chunk(&mut output, chunk_id, value.as_bytes())?;
        }

        // Convert iXML to AIFF comment chunks
        for xml in &ixml_chunks {
            // Parse iXML and create COMT chunk
            let mut comment_data = Vec::new();
            comment_data.write_u16::<BigEndian>(1)?; // Number of comments

            // Single comment entry: timestamp + marker_id + count + text
            comment_data.write_u32::<BigEndian>(0)?; // Timestamp
            comment_data.write_u16::<BigEndian>(0)?; // Marker ID
            comment_data.write_u16::<BigEndian>(xml.len() as u16)?; // Text length
            comment_data.extend_from_slice(xml.as_bytes());

            // Add padding if needed
            if xml.len() % 2 == 1 {
                comment_data.push(0);
            }

            aiff_write_chunk(&mut output, COMT_CHUNK_ID, &comment_data)?;
        }

        // Write ID3 chunks
        for id3_data in &id3_chunks {
            aiff_write_chunk(&mut output, ID3_CHUNK_ID, id3_data)?;
        }

        // Write other unknown chunks
        for (id, data) in &other_chunks {
            if id.len() == 4 {
                let id_bytes = id.as_bytes();
                if id_bytes.len() == 4 {
                    let chunk_id: [u8; 4] = [id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]];
                    aiff_write_chunk(&mut output, &chunk_id, data)?;
                }
            }
        }

        // Update FORM chunk size
        let final_size = output.position() as u32 - 8;
        let output_data = output.into_inner();
        let mut result_data = output_data.clone();
        (&mut result_data[4..8]).write_u32::<BigEndian>(final_size)?;

        Ok(result_data)
    }
}

fn aiff_write_chunk<W: Write>(writer: &mut W, id: &[u8], data: &[u8]) -> R<()> {
    writer.write_all(id)?;
    writer.write_u32::<BigEndian>(data.len() as u32)?;
    writer.write_all(data)?;
    if data.len() % 2 == 1 {
        writer.write_all(&[0])?; // padding
    }
    Ok(())
}

fn decode_samples(
    input: &[u8],
    channels: u16,
    bits_per_sample: u16,
    is_float_format: bool,
) -> R<Vec<Vec<f32>>> {
    let bytes_per_sample = match bits_per_sample {
        8 => 1,
        16 => 2,
        24 => 3,
        32 => 4,
        _ => return Err(anyhow!("Unsupported bit depth")),
    };

    let samples_per_channel = input.len() / (channels as usize * bytes_per_sample);

    let output: Vec<Vec<f32>> = (0..channels as usize)
        .into_par_iter() // Parallelize over channels
        .map(|ch| {
            let mut channel_data = vec![0.0; samples_per_channel];

            #[allow(clippy::needless_range_loop)]
            for i in 0..samples_per_channel {
                let pos = i * channels as usize + ch;
                let sample_idx = pos * bytes_per_sample;

                if sample_idx + bytes_per_sample - 1 < input.len() {
                    let val = match bits_per_sample {
                        8 => {
                            // AIFF 8-bit samples are signed, unlike WAV which uses unsigned
                            input[sample_idx] as i8 as f32 / 127.0
                        }
                        16 => {
                            let val =
                                i16::from_be_bytes([input[sample_idx], input[sample_idx + 1]]);
                            val as f32 / I16_DIVISOR
                        }
                        24 => {
                            // For AIFF (big-endian), the bytes are in big-endian order
                            // MSB first: [MSB] [MID] [LSB]
                            let val = ((input[sample_idx] as i32) << 16)
                                | ((input[sample_idx + 1] as i32) << 8)
                                | (input[sample_idx + 2] as i32);
                            // Sign extend from 24-bit to 32-bit
                            let val = if val & I24_SIGN_BIT != 0 {
                                val | I24_SIGN_EXTENSION_MASK
                            } else {
                                val
                            };
                            val as f32 / I24_DIVISOR
                        }
                        32 => {
                            if is_float_format {
                                let bytes = [
                                    input[sample_idx],
                                    input[sample_idx + 1],
                                    input[sample_idx + 2],
                                    input[sample_idx + 3],
                                ];
                                f32::from_be_bytes(bytes)
                            } else {
                                let val = i32::from_be_bytes([
                                    input[sample_idx],
                                    input[sample_idx + 1],
                                    input[sample_idx + 2],
                                    input[sample_idx + 3],
                                ]);
                                val as f32 / I32_DIVISOR
                            }
                        }
                        _ => return vec![],
                    };
                    channel_data[i] = val;
                }
            }

            channel_data
        })
        .collect();

    Ok(output)
}

fn encode_samples<W: Write>(out: &mut W, buffer: &AudioBuffer, bits_per_sample: u16) -> R<()> {
    let channels = buffer.channels as usize;
    let frames = buffer.data[0].len();

    for i in 0..frames {
        for ch in 0..channels {
            let sample = buffer.data[ch][i];
            match bits_per_sample {
                8 => {
                    // AIFF 8-bit samples are signed
                    let val = (sample.clamp(-1.0, 1.0) * 127.0) as i8;
                    out.write_i8(val)?;
                }
                16 => {
                    let val = (sample.clamp(-1.0, 1.0) * I16_MAX_F) as i16;
                    out.write_i16::<BigEndian>(val)?;
                }
                24 => {
                    let val = (sample.clamp(-1.0, 1.0) * I24_MAX_F) as i32;
                    // For big-endian, we need to write the most significant bytes first
                    let bytes = [
                        ((val >> 16) & BYTE_MASK) as u8,
                        ((val >> 8) & BYTE_MASK) as u8,
                        (val & BYTE_MASK) as u8,
                    ];
                    out.write_all(&bytes)?;
                }
                32 => {
                    if buffer.format == SampleFormat::F32 {
                        out.write_f32::<BigEndian>(sample)?;
                    } else {
                        let val = (sample.clamp(-1.0, 1.0) * I32_MAX_F) as i32;
                        out.write_i32::<BigEndian>(val)?;
                    }
                }
                _ => return Err(anyhow!("Unsupported bit depth")),
            }
        }
    }

    Ok(())
}

// Helper function to write IEEE 80-bit extended float (required for AIFF)
fn write_ieee_extended<W: Write>(writer: &mut W, mut value: f64) -> R<()> {
    let mut buffer = [0u8; 10];

    // Handle special cases first
    if value.is_nan() {
        // NaN representation
        buffer[0] = 0x7F;
        buffer[1] = 0xFF;
        buffer[2] = 0xFF; // Set first mantissa bit to indicate NaN
        return writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e));
    }

    if value.is_infinite() {
        // Infinity representation
        buffer[0] = if value.is_sign_negative() { 0xFF } else { 0x7F };
        buffer[1] = 0xFF;
        buffer[2] = 0x80; // Explicit leading bit for infinity
        return writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e));
    }

    // Handle sign
    if value < 0.0 {
        buffer[0] = 0x80;
        value = -value;
    } else {
        buffer[0] = 0;
    }

    // Handle zero
    if value == 0.0 {
        return writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e));
    }

    // Compute exponent and mantissa
    let mut exponent: i16 = 16383; // Bias for 80-bit IEEE

    // Normalize the number
    let mut fraction = value;
    while fraction >= 2.0 {
        fraction /= 2.0;
        exponent += 1;
    }

    while fraction < 1.0 {
        fraction *= 2.0;
        exponent -= 1;
    }

    // Check for exponent overflow/underflow
    if exponent > 0x7FFE {
        // Represent as infinity
        buffer[0] |= 0x7F;
        buffer[1] = 0xFF;
        buffer[2] = 0x80;
        return writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e));
    }

    if exponent < 0 {
        // Represent as zero for underflow
        return writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e));
    }

    // Convert to fixed point mantissa
    // For 80-bit IEEE, the leading bit is explicit
    let mantissa_bits = (fraction * (1u64 << 63) as f64) as u64;

    // Fill exponent in buffer
    buffer[0] |= ((exponent >> 8) & 0x7F) as u8;
    buffer[1] = (exponent & 0xFF) as u8;

    // Fill the mantissa - ensure correct byte order (big endian)
    buffer[2] = ((mantissa_bits >> 56) & 0xFF) as u8;
    buffer[3] = ((mantissa_bits >> 48) & 0xFF) as u8;
    buffer[4] = ((mantissa_bits >> 40) & 0xFF) as u8;
    buffer[5] = ((mantissa_bits >> 32) & 0xFF) as u8;
    buffer[6] = ((mantissa_bits >> 24) & 0xFF) as u8;
    buffer[7] = ((mantissa_bits >> 16) & 0xFF) as u8;
    buffer[8] = ((mantissa_bits >> 8) & 0xFF) as u8;
    buffer[9] = (mantissa_bits & 0xFF) as u8;

    writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e))
}

// Helper function to read IEEE 80-bit extended float (required for AIFF)
fn read_ieee_extended<E: Read>(reader: &mut E) -> R<f64> {
    let mut buffer = [0u8; 10];
    reader.read_exact(&mut buffer)?;

    // Extract sign
    let sign = if buffer[0] & 0x80 != 0 { -1.0 } else { 1.0 };

    // Extract exponent
    let exponent = (((buffer[0] as u16) & 0x7F) << 8) | (buffer[1] as u16);

    // Handle special cases
    if exponent == 0 && buffer[2..].iter().all(|&b| b == 0) {
        return Ok(0.0);
    }

    // Handle infinity and NaN
    if exponent == 0x7FFF {
        if buffer[2..].iter().all(|&b| b == 0) {
            return Ok(if sign == 1.0 {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            });
        } else {
            return Ok(f64::NAN);
        }
    }

    // Extract mantissa (IEEE 80-bit has explicit leading bit)
    let mut mantissa: f64 = 0.0;
    let mut bit_value = 1.0; // Start with implicit leading 1

    // First bit of mantissa is explicit in 80-bit format
    if buffer[2] & 0x80 != 0 {
        mantissa += bit_value;
    }
    bit_value *= 0.5;

    // Process remaining mantissa bits
    for &byte in &buffer[2..] {
        for bit_pos in (0..8).rev() {
            if bit_pos == 7 && byte == buffer[2] {
                continue; // Skip the explicit leading bit we already processed
            }
            if byte & (1 << bit_pos) != 0 {
                mantissa += bit_value;
            }
            bit_value *= 0.5;
        }
    }

    // Apply bias and scale - IEEE 80-bit uses bias of 16383
    let real_exponent = exponent as i32 - 16383;

    // Prevent overflow/underflow
    if real_exponent > 1023 {
        return Ok(if sign == 1.0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        });
    }
    if real_exponent < -1022 {
        return Ok(0.0);
    }

    // Calculate final value
    let value = sign * mantissa * 2.0f64.powi(real_exponent);

    Ok(value)
}

// A simpler, more direct implementation for common sample rates
fn write_ieee_extended_simple<W: Write>(writer: &mut W, value: f64) -> R<()> {
    // For common audio sample rates, use precomputed values
    let buffer: [u8; 10] = match value as u32 {
        44100 => [0x40, 0x0E, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        48000 => [0x40, 0x0E, 0xBB, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        88200 => [0x40, 0x0F, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        96000 => [0x40, 0x0F, 0xBB, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => {
            // Fall back to general implementation for uncommon rates
            let mut buf = [0u8; 10];
            let mut cursor = Cursor::new(&mut buf[..]);
            write_ieee_extended(&mut cursor, value)?;
            buf
        }
    };

    writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e))
}
