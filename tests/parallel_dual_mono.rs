use rayon::prelude::*;
use std::io::Write;

// 16-bit stereo WAV where L == R and every sample equals `marker`.
fn write_dual_mono_wav(path: &std::path::Path, marker: i16, frames: usize) {
    let data_len = (frames * 4) as u32;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    f.write_all(&2u16.to_le_bytes()).unwrap(); // channels
    f.write_all(&48000u32.to_le_bytes()).unwrap();
    f.write_all(&(48000u32 * 4).to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    let mut buf = Vec::with_capacity(data_len as usize);
    for _ in 0..frames {
        buf.extend_from_slice(&marker.to_le_bytes());
        buf.extend_from_slice(&marker.to_le_bytes());
    }
    f.write_all(&buf).unwrap();
}

// Returns (channels, first sample) by walking RIFF chunks.
fn read_wav(path: &std::path::Path) -> (u16, i16) {
    let b = std::fs::read(path).unwrap();
    let mut i = 12;
    let mut ch = 0;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"fmt " {
            ch = u16::from_le_bytes(b[i + 10..i + 12].try_into().unwrap());
        }
        if id == b"data" {
            return (ch, i16::from_le_bytes(b[i + 8..i + 10].try_into().unwrap()));
        }
        i += 8 + len + (len & 1);
    }
    panic!("no data chunk in {}", path.display());
}

#[test]
fn parallel_conversion_keeps_each_files_own_audio() {
    let root = tempfile::tempdir().unwrap();
    let n = 64;
    // Mix of unique names and repeated names in different folders (common in libraries)
    let paths: Vec<_> = (0..n)
        .map(|i| {
            let dir = root.path().join(format!("folder{}", i));
            std::fs::create_dir_all(&dir).unwrap();
            let name = if i % 2 == 0 { format!("TOYMisc_{}.wav", i) } else { "Same Name.wav".to_string() };
            let p = dir.join(name);
            write_dual_mono_wav(&p, 100 + i as i16, 48000);
            p
        })
        .collect();

    let errors: Vec<String> = paths
        .par_iter()
        .filter_map(|p| ffcodex_lib::clean_multi_mono(p.to_str().unwrap()).err().map(|e| format!("{}: {}", p.display(), e)))
        .collect();

    let mut wrong = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let (ch, first) = read_wav(p);
        if ch != 1 || (first - (100 + i as i16)).abs() > 1 {
            wrong.push(format!("{} -> channels {}, marker {} (expected {})", p.display(), ch, first, 100 + i));
        }
    }
    let leftovers: Vec<_> = walkdir_tmp(root.path());
    assert!(
        errors.is_empty() && wrong.is_empty() && leftovers.is_empty(),
        "{} conversions failed, {} files have wrong audio, {} temp files left behind\n{}",
        errors.len(), wrong.len(), leftovers.len(), wrong.join("\n")
    );
}

fn walkdir_tmp(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for d in std::fs::read_dir(root).unwrap() {
        for f in std::fs::read_dir(d.unwrap().path()).unwrap() {
            let p = f.unwrap().path();
            if p.to_string_lossy().contains(".ffcodex-") { out.push(p); }
        }
    }
    out
}
