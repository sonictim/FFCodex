use crate::prelude::*;

// Format tags
// const FORMAT_PCM: u16 = 1;
// const FORMAT_IEEE_FLOAT: u16 = 3;

// Chunk Identifiers
const FORM_CHUNK_ID: &[u8; 4] = b"FORM";
const AIFF_FORMAT_ID: &[u8; 4] = b"AIFF";
const FMT_CHUNK_ID: &[u8; 4] = b"COMM";
const DATA_CHUNK_ID: &[u8; 4] = b"SSND";

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

    fn encode(&self, buffer: &AudioBuffer) -> R<Vec<u8>> {
        let mut output = Cursor::new(Vec::new());

        // Write FORM header
        output.write_all(FORM_CHUNK_ID)?;
        output.write_u32::<BigEndian>(0)?; // Placeholder for file size
        output.write_all(AIFF_FORMAT_ID)?;

        // Write COMM chunk
        output.write_all(FMT_CHUNK_ID)?;
        output.write_u32::<BigEndian>(18)?; // COMM chunk size
        output.write_u16::<BigEndian>(buffer.channels)?;

        // Write number of sample frames
        let num_frames = if buffer.data.is_empty() {
            0
        } else {
            buffer.data[0].len() as u32
        };
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
            let mut chunk_id = [0u8; 4];
            cursor.read_exact(&mut chunk_id)?;
            let chunk_size = cursor.read_u32::<BigEndian>()? as usize;

            match &chunk_id {
                FMT_CHUNK_ID => {
                    fmt_chunk_found = true;
                    channels = cursor.read_u16::<BigEndian>()?;
                    let _frames = cursor.read_u32::<BigEndian>()?; // Total frames - we read but don't use
                    bits_per_sample = cursor.read_u16::<BigEndian>()?;

                    // Use read_ieee_extended to get the sample rate (80-bit extended precision)
                    sample_rate = read_ieee_extended(&mut cursor)? as u32;

                    sample_format = match bits_per_sample {
                        8 => SampleFormat::U8,
                        16 => SampleFormat::I16,
                        24 => SampleFormat::I24,
                        32 => SampleFormat::I32,
                        _ => {
                            return Err(anyhow!("Unsupported bit depth: {}", bits_per_sample));
                        }
                    };
                }

                DATA_CHUNK_ID => {
                    data_chunk_found = true;
                    cursor.read_u32::<BigEndian>()?; // Offset
                    cursor.read_u32::<BigEndian>()?; // Block size

                    let mut raw_data = vec![0u8; chunk_size - 8];
                    cursor.read_exact(&mut raw_data)?;

                    audio_data = decode_samples(
                        &raw_data,
                        channels,
                        bits_per_sample,
                        sample_format == SampleFormat::F32,
                    )?;
                }

                _ => {
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
                        8 => input[sample_idx] as f32 / U8_SCALE - 1.0,
                        16 => {
                            let val =
                                i16::from_be_bytes([input[sample_idx], input[sample_idx + 1]]);
                            val as f32 / I16_DIVISOR
                        }
                        24 => {
                            let val = ((input[sample_idx] as i32) << 16)
                                | ((input[sample_idx + 1] as i32) << 8)
                                | (input[sample_idx + 2] as i32);
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
                    let val = ((sample * U8_SCALE + U8_OFFSET).clamp(0.0, 255.0)) as u8;
                    out.write_u8(val)?;
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

    if value < 0.0 {
        buffer[0] = 0x80;
        value = -value;
    } else {
        buffer[0] = 0;
    }

    // Handle special cases
    if value == 0.0 {
        return writer.write_all(&buffer).map_err(|e| anyhow::anyhow!(e));
    }

    // Compute exponent and mantissa
    let mut exponent: i16 = 16383; // Bias

    // Get normalized fraction and exponent
    let mut fraction = value;
    while fraction >= 1.0 {
        fraction /= 2.0;
        exponent += 1;
    }

    while fraction < 0.5 {
        fraction *= 2.0;
        exponent -= 1;
    }

    // Convert to fixed point mantissa
    fraction *= 2.0; // Shift left to get 1.fraction
    let mantissa: u64 = ((fraction - 1.0) * 9007199254740992.0) as u64; // 2^53, corrected to subtract implicit 1

    // Fill buffer
    buffer[0] |= ((exponent >> 8) & 0x7F) as u8;
    buffer[1] = (exponent & 0xFF) as u8;

    // Fill the mantissa - ensure correct byte order (big endian)
    buffer[2] = ((mantissa >> 56) & 0xFF) as u8;
    buffer[3] = ((mantissa >> 48) & 0xFF) as u8;
    buffer[4] = ((mantissa >> 40) & 0xFF) as u8;
    buffer[5] = ((mantissa >> 32) & 0xFF) as u8;
    buffer[6] = ((mantissa >> 24) & 0xFF) as u8;
    buffer[7] = ((mantissa >> 16) & 0xFF) as u8;
    buffer[8] = ((mantissa >> 8) & 0xFF) as u8;
    buffer[9] = (mantissa & 0xFF) as u8;

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

    // Extract mantissa (first bit is implicit 1)
    let mut mantissa: f64 = 0.0;
    let mut bit_value = 0.5; // Start with 2^-1

    for &byte in &buffer[2..] {
        for bit_pos in (0..8).rev() {
            if byte & (1 << bit_pos) != 0 {
                mantissa += bit_value;
            }
            bit_value *= 0.5;
        }
    }

    // Add the implicit leading 1
    mantissa += 1.0;

    // Apply bias, sign, and scale
    let real_exponent = exponent as i32 - 16383;

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
