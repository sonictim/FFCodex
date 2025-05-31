use std::fs;
use ffcodex::prelude::*;
use ffcodex::codecs::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple test audio buffer
    let sample_rate = 44100;
    let channels = 2;
    let duration_secs = 1;
    let samples = sample_rate * duration_secs;
    
    // Generate a simple sine wave
    let mut left = Vec::new();
    let mut right = Vec::new();
    
    for i in 0..samples {
        let t = i as f32 / sample_rate as f32;
        let sample_l = (440.0 * 2.0 * std::f32::consts::PI * t).sin() * 0.5;
        let sample_r = (880.0 * 2.0 * std::f32::consts::PI * t).sin() * 0.5;
        left.push(sample_l);
        right.push(sample_r);
    }
    
    let audio_buffer = AudioBuffer {
        sample_rate,
        channels,
        format: SampleFormat::I16,
        data: vec![left, right],
    };
    
    // Create metadata
    let mut metadata_chunks = Vec::new();
    metadata_chunks.push(MetadataChunk::TextTag {
        key: "TITLE".to_string(),
        value: "Test Song".to_string(),
    });
    metadata_chunks.push(MetadataChunk::TextTag {
        key: "ARTIST".to_string(),
        value: "Test Artist".to_string(),
    });
    metadata_chunks.push(MetadataChunk::TextTag {
        key: "ALBUM".to_string(),
        value: "Test Album".to_string(),
    });
    
    let metadata = Metadata::Wav(metadata_chunks);
    
    // Encode to WavPack without metadata first
    let wv_codec = WvCodec;
    let encoded_data = wv_codec.encode(&audio_buffer)?;
    fs::write("test_no_metadata.wv", &encoded_data)?;
    println!("Created test_no_metadata.wv ({} bytes)", encoded_data.len());
    
    // Now embed metadata
    let final_data = wv_codec.embed_metadata_chunks(&encoded_data, &metadata.chunks())?;
    fs::write("test_with_metadata.wv", &final_data)?;
    println!("Created test_with_metadata.wv ({} bytes)", final_data.len());
    
    // Verify the metadata in the final file
    let extracted_metadata = wv_codec.extract_metadata_chunks(&final_data)?;
    println!("Extracted {} metadata chunks from final file:", extracted_metadata.len());
    for chunk in &extracted_metadata {
        match chunk {
            MetadataChunk::TextTag { key, value } => {
                println!("  Text tag: {} = {}", key, value);
            }
            MetadataChunk::Picture { mime_type, description, data } => {
                println!("  Picture: {} ({}) - {} bytes", description, mime_type, data.len());
            }
            _ => {
                println!("  Other: {}", chunk.id());
            }
        }
    }
    
    Ok(())
}
