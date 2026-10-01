//! Lossless dual/multi-mono stripping for WAV and AIFF.
//!
//! Instead of decoding to float and re-encoding, this keeps the first channel's
//! bytes from every frame and copies every other chunk (bext, iXML, SMED, cue,
//! smpl, LIST, XMP, ...) through byte-for-byte. Only the channel count fields
//! in `fmt `/`COMM` change. This is bit-exact for every PCM and float format.

use crate::prelude::*;

/// Largest per-sample difference between channels (full scale = 1.0) that is
/// still treated as "identical". Matches the loosest tolerance the dual mono
/// search uses, so files flagged by the search are accepted, while a stale flag
/// or a genuinely stereo file is refused instead of losing its other channels.
pub const MAX_CHANNEL_DIFF: f64 = 1e-4;

#[derive(Clone, Copy)]
enum Sample {
    /// Integer sample in a `bytes`-wide container (valid bits are top-aligned)
    Int { bytes: usize, little_endian: bool, unsigned_8bit: bool },
    F32 { little_endian: bool },
    F64 { little_endian: bool },
}

impl Sample {
    fn bytes(&self) -> usize {
        match *self {
            Sample::Int { bytes, .. } => bytes,
            Sample::F32 { .. } => 4,
            Sample::F64 { .. } => 8,
        }
    }

    /// Reads one sample normalized to roughly -1.0..1.0
    fn read(&self, b: &[u8]) -> f64 {
        match *self {
            Sample::Int { bytes, little_endian, unsigned_8bit } => {
                if unsigned_8bit {
                    return (b[0] as f64 - 128.0) / 128.0;
                }
                let mut v: i64 = 0;
                for i in 0..bytes {
                    let byte = if little_endian { b[bytes - 1 - i] } else { b[i] };
                    v = (v << 8) | byte as i64;
                }
                // Sign-extend from the container width, then scale to full scale
                let shift = 64 - 8 * bytes as u32;
                let v = (v << shift) >> shift;
                v as f64 / (1u64 << (8 * bytes as u32 - 1)) as f64
            }
            Sample::F32 { little_endian } => {
                let a = [b[0], b[1], b[2], b[3]];
                (if little_endian { f32::from_le_bytes(a) } else { f32::from_be_bytes(a) }) as f64
            }
            Sample::F64 { little_endian } => {
                let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
                if little_endian { f64::from_le_bytes(a) } else { f64::from_be_bytes(a) }
            }
        }
    }
}

/// Keeps channel 0 of each frame after checking all channels match it.
fn strip_frames(data: &[u8], channels: usize, sample: Sample, frames: usize) -> R<Vec<u8>> {
    let bps = sample.bytes();
    let frame_len = bps * channels;
    let mut out = Vec::with_capacity(frames * bps);
    for f in 0..frames {
        let frame = &data[f * frame_len..(f + 1) * frame_len];
        let first = &frame[..bps];
        for ch in 1..channels {
            let other = &frame[ch * bps..(ch + 1) * bps];
            if other != first {
                let diff = (sample.read(first) - sample.read(other)).abs();
                if !(diff <= MAX_CHANNEL_DIFF) {
                    return Err(anyhow!(
                        "Channels are not identical (frame {}, channel {}); refusing to strip",
                        f,
                        ch + 1
                    ));
                }
            }
        }
        out.extend_from_slice(first);
    }
    Ok(out)
}

struct Chunk<'a> {
    id: [u8; 4],
    data: &'a [u8],
}

fn read_chunks(input: &[u8], big_endian: bool) -> R<Vec<Chunk<'_>>> {
    let mut chunks = Vec::new();
    let mut pos = 12;
    while pos + 8 <= input.len() {
        let id = [input[pos], input[pos + 1], input[pos + 2], input[pos + 3]];
        let size_bytes = [input[pos + 4], input[pos + 5], input[pos + 6], input[pos + 7]];
        let size = if big_endian {
            u32::from_be_bytes(size_bytes)
        } else {
            u32::from_le_bytes(size_bytes)
        } as usize;
        let start = pos + 8;
        // Tolerate a truncated final chunk (common with interrupted recorders)
        let end = (start + size).min(input.len());
        chunks.push(Chunk { id, data: &input[start..end] });
        pos = start + size + (size & 1);
    }
    Ok(chunks)
}

fn write_chunks(form: &[u8; 4], kind: &[u8; 4], chunks: &[(&[u8; 4], std::borrow::Cow<[u8]>)], big_endian: bool) -> Vec<u8> {
    let body_len: usize = 4 + chunks.iter().map(|(_, d)| 8 + d.len() + (d.len() & 1)).sum::<usize>();
    let mut out = Vec::with_capacity(8 + body_len);
    let put_u32 = |out: &mut Vec<u8>, v: u32| {
        out.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() })
    };
    out.extend_from_slice(form);
    put_u32(&mut out, body_len as u32);
    out.extend_from_slice(kind);
    for (id, data) in chunks {
        out.extend_from_slice(*id);
        put_u32(&mut out, data.len() as u32);
        out.extend_from_slice(data);
        if data.len() & 1 == 1 {
            out.push(0);
        }
    }
    out
}

pub fn strip_wav(input: &[u8]) -> R<Vec<u8>> {
    if input.len() < 12 || &input[0..4] != b"RIFF" || &input[8..12] != b"WAVE" {
        return Err(anyhow!("Not a RIFF/WAVE file"));
    }
    let chunks = read_chunks(input, false)?;
    let fmt = chunks
        .iter()
        .find(|c| &c.id == b"fmt ")
        .ok_or_else(|| anyhow!("Missing fmt chunk"))?
        .data;
    if fmt.len() < 16 {
        return Err(anyhow!("fmt chunk too small"));
    }
    let u16_at = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
    let mut format_tag = u16_at(0);
    let channels = u16_at(2) as usize;
    let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    let block_align = u16_at(12) as usize;
    if format_tag == 0xFFFE && fmt.len() >= 40 {
        format_tag = u16_at(24); // first two bytes of the SubFormat GUID
    }
    if channels < 2 || block_align == 0 || block_align % channels != 0 {
        return Err(anyhow!("Unsupported channel layout ({} channels)", channels));
    }
    let bps = block_align / channels;
    let sample = match (format_tag, bps) {
        (1, 1) => Sample::Int { bytes: 1, little_endian: true, unsigned_8bit: true },
        (1, 2..=4) => Sample::Int { bytes: bps, little_endian: true, unsigned_8bit: false },
        (3, 4) => Sample::F32 { little_endian: true },
        (3, 8) => Sample::F64 { little_endian: true },
        _ => return Err(anyhow!("Unsupported WAV format {} with {} bytes/sample", format_tag, bps)),
    };

    let mut out_chunks = Vec::with_capacity(chunks.len());
    for c in &chunks {
        match &c.id {
            b"fmt " => {
                let mut f = c.data.to_vec();
                f[2..4].copy_from_slice(&1u16.to_le_bytes());
                f[8..12].copy_from_slice(&(sample_rate * bps as u32).to_le_bytes());
                f[12..14].copy_from_slice(&(bps as u16).to_le_bytes());
                if u16_at(0) == 0xFFFE && f.len() >= 24 {
                    f[20..24].copy_from_slice(&4u32.to_le_bytes()); // front center
                }
                out_chunks.push((b"fmt ", std::borrow::Cow::Owned(f)));
            }
            b"data" => {
                let frames = c.data.len() / block_align;
                let stripped = strip_frames(c.data, channels, sample, frames)?;
                out_chunks.push((b"data", std::borrow::Cow::Owned(stripped)));
            }
            _ => out_chunks.push((&c.id, std::borrow::Cow::Borrowed(c.data))),
        }
    }
    if !chunks.iter().any(|c| &c.id == b"data") {
        return Err(anyhow!("Missing data chunk"));
    }
    Ok(write_chunks(b"RIFF", b"WAVE", &out_chunks, false))
}

pub fn strip_aiff(input: &[u8]) -> R<Vec<u8>> {
    if input.len() < 12 || &input[0..4] != b"FORM" {
        return Err(anyhow!("Not an IFF FORM file"));
    }
    let kind: [u8; 4] = [input[8], input[9], input[10], input[11]];
    let is_aifc = match &kind {
        b"AIFF" => false,
        b"AIFC" => true,
        _ => return Err(anyhow!("Not an AIFF/AIFC file")),
    };
    let chunks = read_chunks(input, true)?;
    let comm = chunks
        .iter()
        .find(|c| &c.id == b"COMM")
        .ok_or_else(|| anyhow!("Missing COMM chunk"))?
        .data;
    if comm.len() < 18 || (is_aifc && comm.len() < 22) {
        return Err(anyhow!("COMM chunk too small"));
    }
    let channels = i16::from_be_bytes([comm[0], comm[1]]);
    let num_frames = u32::from_be_bytes([comm[2], comm[3], comm[4], comm[5]]) as usize;
    let bits = i16::from_be_bytes([comm[6], comm[7]]);
    if channels < 2 || !(1..=32).contains(&bits) {
        return Err(anyhow!("Unsupported AIFF layout ({} channels, {} bits)", channels, bits));
    }
    let channels = channels as usize;
    let int_bytes = (bits as usize).div_ceil(8);
    let int = |le: bool| Sample::Int { bytes: int_bytes, little_endian: le, unsigned_8bit: false };
    let sample = if is_aifc {
        match &[comm[18], comm[19], comm[20], comm[21]] {
            b"NONE" | b"twos" | b"in24" | b"in32" => int(false),
            b"sowt" | b"42ni" | b"23ni" => int(true),
            b"fl32" | b"FL32" => Sample::F32 { little_endian: false },
            b"fl64" | b"FL64" => Sample::F64 { little_endian: false },
            other => {
                return Err(anyhow!(
                    "Unsupported AIFC compression {:?}",
                    String::from_utf8_lossy(other)
                ));
            }
        }
    } else {
        int(false)
    };
    let frame_len = sample.bytes() * channels;

    let mut out_chunks = Vec::with_capacity(chunks.len());
    for c in &chunks {
        match &c.id {
            b"COMM" => {
                let mut f = c.data.to_vec();
                f[0..2].copy_from_slice(&1i16.to_be_bytes());
                out_chunks.push((b"COMM", std::borrow::Cow::Owned(f)));
            }
            b"SSND" => {
                if c.data.len() < 8 {
                    return Err(anyhow!("SSND chunk too small"));
                }
                let offset = u32::from_be_bytes([c.data[0], c.data[1], c.data[2], c.data[3]]) as usize;
                let header_len = (8 + offset).min(c.data.len());
                let audio = &c.data[header_len..];
                let frames = num_frames.min(audio.len() / frame_len);
                let mut s = c.data[..header_len].to_vec();
                s.extend(strip_frames(audio, channels, sample, frames)?);
                out_chunks.push((b"SSND", std::borrow::Cow::Owned(s)));
            }
            _ => out_chunks.push((&c.id, std::borrow::Cow::Borrowed(c.data))),
        }
    }
    if !chunks.iter().any(|c| &c.id == b"SSND") {
        return Err(anyhow!("Missing SSND chunk"));
    }
    Ok(write_chunks(b"FORM", &kind, &out_chunks, true))
}
