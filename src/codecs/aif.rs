use crate::prelude::*;

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
const IXML_CHUNK_ID: &[u8; 4] = b"iXML";

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
        use memmap2::MmapOptions;
        use std::fs;

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
        let mut description = String::new();

        // Find the COMM chunk and description chunks
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

            match &chunk_id {
                FMT_CHUNK_ID => {
                    // Found COMM chunk - extract format information
                    channels = cursor.read_u16::<BigEndian>()?;
                    total_frames = cursor.read_u32::<BigEndian>()?;
                    bits_per_sample = cursor.read_u16::<BigEndian>()?;

                    // Read the 80-bit IEEE extended sample rate
                    sample_rate = read_ieee_extended(&mut cursor)? as u16;
                }
                ANNO_CHUNK_ID => {
                    // AIFF annotation chunk contains description
                    if description.is_empty() {
                        let mut anno_data = vec![0u8; chunk_size];
                        cursor.read_exact(&mut anno_data)?;
                        description = String::from_utf8_lossy(&anno_data)
                            .trim_end_matches('\0')
                            .trim()
                            .to_string();
                    } else {
                        cursor.set_position(cursor.position() + chunk_size as u64);
                    }
                }
                COMT_CHUNK_ID => {
                    // AIFF comment chunk - extract first comment as description
                    if description.is_empty() && chunk_size >= 2 {
                        let num_comments = cursor.read_u16::<BigEndian>()? as usize;
                        let mut remaining_size = chunk_size - 2;

                        if num_comments > 0 && remaining_size >= 8 {
                            // Skip timestamp (4 bytes) and marker_id (2 bytes)
                            cursor.read_u32::<BigEndian>()?; // timestamp
                            cursor.read_u16::<BigEndian>()?; // marker_id
                            let text_len = cursor.read_u16::<BigEndian>()? as usize;
                            remaining_size -= 8;

                            if text_len > 0 && text_len <= remaining_size {
                                let mut comment_data = vec![0u8; text_len];
                                cursor.read_exact(&mut comment_data)?;
                                description =
                                    String::from_utf8_lossy(&comment_data).trim().to_string();
                                remaining_size -= text_len;
                            }
                        }

                        // Skip any remaining data
                        if remaining_size > 0 {
                            cursor.set_position(cursor.position() + remaining_size as u64);
                        }
                    } else {
                        cursor.set_position(cursor.position() + chunk_size as u64);
                    }
                }
                _ => {
                    // Skip other chunks
                    cursor.set_position(cursor.position() + chunk_size as u64);
                }
            }

            // Handle padding
            if chunk_size % 2 == 1 {
                cursor.set_position(cursor.position() + 1);
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
            sample_rate,
            channels,
            bit_depth: bits_per_sample,
            duration,
            description,
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

    fn parse_metadata(&self, input: &[u8]) -> R<Metadata> {
        let mut metadata = Metadata::new();
        let mut cursor = Cursor::new(input);

        // Validate AIFF header
        let mut header = [0u8; 12];
        cursor.read_exact(&mut header)?;

        if &header[0..4] != b"FORM" || &header[8..12] != b"AIFF" {
            return Err(anyhow!("Invalid AIFF header"));
        }

        // Parse chunks
        while cursor.position() < input.len() as u64 {
            // Read chunk header
            let chunk_id = match cursor.read_u32::<BigEndian>() {
                Ok(id) => id,
                Err(_) => break,
            };

            let chunk_size = match cursor.read_u32::<BigEndian>() {
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
            match &chunk_id.to_be_bytes() {
                b"NAME" => {
                    // Name chunk - contains title
                    let name_str = String::from_utf8_lossy(chunk_data);
                    let name = name_str.trim_end_matches('\0').trim();
                    if !name.is_empty() {
                        metadata.set_field("Title", name)?;
                    }
                }
                b"AUTH" => {
                    // Author chunk - contains artist
                    let author_str = String::from_utf8_lossy(chunk_data);
                    let author = author_str.trim_end_matches('\0').trim();
                    if !author.is_empty() {
                        metadata.set_field("Artist", author)?;
                    }
                }
                b"(c) " => {
                    // Copyright chunk
                    let copyright_str = String::from_utf8_lossy(chunk_data);
                    let copyright = copyright_str.trim_end_matches('\0').trim();
                    if !copyright.is_empty() {
                        metadata.set_field("Copyright", copyright)?;
                    }
                }
                b"ANNO" => {
                    // Annotation chunk - contains comments
                    let annotation_str = String::from_utf8_lossy(chunk_data);
                    let annotation = annotation_str.trim_end_matches('\0').trim();
                    if !annotation.is_empty() {
                        metadata.set_field("Comment", annotation)?;
                    }
                }
                b"iXML" => {
                    // iXML chunk
                    let xml_str = String::from_utf8_lossy(chunk_data);
                    metadata.parse_ixml(&xml_str)?;
                }
                b"ID3 " | b"id3 " => {
                    // ID3 chunk
                    metadata.parse_id3(chunk_data)?;
                }
                _ => {
                    // Check if it's a text chunk (4 printable ASCII characters)
                    let chunk_id_bytes = chunk_id.to_be_bytes();
                    let chunk_id_str_val = String::from_utf8_lossy(&chunk_id_bytes);
                    let chunk_id_str = chunk_id_str_val.trim();
                    if chunk_id_str
                        .chars()
                        .all(|c| c.is_ascii_graphic() || c == ' ')
                    {
                        let text_value_str = String::from_utf8_lossy(chunk_data);
                        let text_value = text_value_str.trim_end_matches('\0').trim();
                        if !text_value.is_empty() {
                            metadata.set_field(chunk_id_str, text_value)?;
                        }
                    }
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

        // Create new data with metadata embedded
        let new_data = self.embed_metadata_from_hashmap(&mapped_file, metadata)?;

        // Write the data back to the file
        std::fs::write(file_path, new_data)?;
        Ok(())
    }
}

impl AifCodec {
    fn embed_metadata_from_hashmap(&self, input: &[u8], metadata: &Metadata) -> R<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut output = Cursor::new(Vec::new());

        // Copy the FORM/AIFF header
        let mut header = [0u8; 12];
        cursor.read_exact(&mut header)?;
        output.write_all(&header)?;

        // Copy fmt and data chunks, skipping old metadata chunks
        let mut fmt_chunk_found = false;
        let mut data_chunk_found = false;

        while cursor.position() < input.len() as u64 {
            let mut chunk_id = [0u8; 4];
            if cursor.read(&mut chunk_id)? < 4 {
                break;
            }

            let chunk_size = cursor.read_u32::<BigEndian>()?;

            match &chunk_id {
                FMT_CHUNK_ID => {
                    fmt_chunk_found = true;
                    // Copy COMM chunk as-is
                    output.write_all(&chunk_id)?;
                    output.write_u32::<BigEndian>(chunk_size)?;

                    let mut chunk_data = vec![0u8; chunk_size as usize];
                    cursor.read_exact(&mut chunk_data)?;
                    output.write_all(&chunk_data)?;

                    // Handle padding
                    if chunk_size % 2 == 1 {
                        cursor.seek(SeekFrom::Current(1))?;
                        output.write_all(&[0])?;
                    }
                }
                DATA_CHUNK_ID => {
                    data_chunk_found = true;
                    // Copy SSND chunk as-is
                    output.write_all(&chunk_id)?;
                    output.write_u32::<BigEndian>(chunk_size)?;

                    let mut chunk_data = vec![0u8; chunk_size as usize];
                    cursor.read_exact(&mut chunk_data)?;
                    output.write_all(&chunk_data)?;

                    // Handle padding
                    if chunk_size % 2 == 1 {
                        cursor.seek(SeekFrom::Current(1))?;
                        output.write_all(&[0])?;
                    }
                }
                // Skip existing metadata chunks - we'll recreate them
                ANNO_CHUNK_ID | COMT_CHUNK_ID | NAME_CHUNK_ID | AUTH_CHUNK_ID
                | COPYRIGHT_CHUNK_ID | APPL_CHUNK_ID | ID3_CHUNK_ID | IXML_CHUNK_ID => {
                    cursor.seek(SeekFrom::Current(chunk_size as i64))?;
                    if chunk_size % 2 == 1 {
                        cursor.seek(SeekFrom::Current(1))?;
                    }
                }
                _ => {
                    // Copy unknown chunks as-is
                    output.write_all(&chunk_id)?;
                    output.write_u32::<BigEndian>(chunk_size)?;

                    let mut chunk_data = vec![0u8; chunk_size as usize];
                    cursor.read_exact(&mut chunk_data)?;
                    output.write_all(&chunk_data)?;

                    // Handle padding
                    if chunk_size % 2 == 1 {
                        cursor.seek(SeekFrom::Current(1))?;
                        output.write_all(&[0])?;
                    }
                }
            }
        }

        if !fmt_chunk_found || !data_chunk_found {
            return Err(anyhow!("Invalid AIFF file: missing COMM or SSND chunk"));
        }

        // Write basic AIFF metadata chunks for common fields
        if let Some(title) = metadata.get_field("Title") {
            self.write_aif_chunk(&mut output, NAME_CHUNK_ID, title.as_bytes())?;
        }
        if let Some(artist) = metadata.get_field("Artist") {
            self.write_aif_chunk(&mut output, AUTH_CHUNK_ID, artist.as_bytes())?;
        }
        if let Some(copyright) = metadata.get_field("Copyright") {
            self.write_aif_chunk(&mut output, COPYRIGHT_CHUNK_ID, copyright.as_bytes())?;
        }
        if let Some(comment) = metadata.get_field("Comment") {
            self.write_aif_chunk(&mut output, ANNO_CHUNK_ID, comment.as_bytes())?;
        }

        // Write comprehensive metadata as iXML chunk (similar to WAV)
        let ixml_content = self.create_ixml_from_metadata(metadata)?;
        self.write_aif_chunk(&mut output, IXML_CHUNK_ID, ixml_content.as_bytes())?;

        // Write image chunks
        for image in metadata.get_images() {
            // Store images as application chunks
            self.write_aif_chunk(&mut output, APPL_CHUNK_ID, image.data())?;
        }

        // Update file size in header
        let final_size = output.position() as u32 - 8;
        let mut result_data = output.into_inner();
        (&mut result_data[4..8]).write_u32::<BigEndian>(final_size)?;

        Ok(result_data)
    }

    fn write_aif_chunk(
        &self,
        output: &mut Cursor<Vec<u8>>,
        chunk_id: &[u8; 4],
        data: &[u8],
    ) -> R<()> {
        output.write_all(chunk_id)?;
        output.write_u32::<BigEndian>(data.len() as u32)?;
        output.write_all(data)?;
        if data.len() % 2 == 1 {
            output.write_all(&[0])?; // padding
        }
        Ok(())
    }

    fn create_ixml_from_metadata(&self, metadata: &Metadata) -> R<String> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<AIFFXML>\n");
        xml.push_str(&crate::ixml::create_ixml_from_metadata(metadata)?);
        xml.push_str("</AIFFXML>\n");
        Ok(xml)
    }
}

// Helper function to read IEEE 754 extended precision numbers (80-bit)
fn read_ieee_extended(cursor: &mut Cursor<&[u8]>) -> R<f64> {
    let mut extended = [0u8; 10];
    cursor.read_exact(&mut extended)?;

    // Extract the sign, exponent, and mantissa
    let sign = (extended[0] & 0x80) != 0;
    let exponent = ((extended[0] as u16 & 0x7F) << 8) | (extended[1] as u16);

    let mut mantissa = 0u64;
    for i in 2..10 {
        mantissa = (mantissa << 8) | (extended[i] as u64);
    }

    // Convert to f64
    if exponent == 0 {
        return Ok(0.0);
    }

    let bias = 16383i32;
    let adjusted_exponent = exponent as i32 - bias;

    // Handle special cases
    if adjusted_exponent > 1023 {
        return Ok(if sign {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }

    let mantissa_f64 = mantissa as f64 / (1u64 << 63) as f64;
    let result = mantissa_f64 * 2.0_f64.powi(adjusted_exponent);

    Ok(if sign { -result } else { result })
}

// Note: aiff_write_chunk function removed as unused

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
