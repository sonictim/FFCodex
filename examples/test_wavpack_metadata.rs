use ffcodex_lib::codecs::*;
use ffcodex_lib::*;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing isolated WavPack metadata embedding...");

    // Read the source file with metadata
    let source_path = "/Users/tfarrell/Desktop/subset test/THND_Fstorm_LUD018.159_shorter.wv";
    println!("Reading source file: {}", source_path);

    if !std::path::Path::new(source_path).exists() {
        return Err(format!("Source file not found: {}", source_path).into());
    }

    let source_data = fs::read(source_path)?;
    println!("Source file size: {} bytes", source_data.len());

    // First, extract and display the metadata from the original file
    let wv_codec = WvCodec;
    println!("\nExtracting metadata from source file...");
    let metadata_chunks =
        if let Ok(original_metadata) = wv_codec.extract_metadata_chunks(&source_data) {
            println!(
                "Found {} metadata chunks in source file:",
                original_metadata.len()
            );
            for (i, chunk) in original_metadata.iter().enumerate() {
                match chunk {
                    MetadataChunk::TextTag { key, value } => {
                        println!("  [{}] Text tag: {} = {}", i, key, value);
                    }
                    MetadataChunk::Picture {
                        mime_type,
                        description,
                        data,
                    } => {
                        println!(
                            "  [{}] Picture: {} ({} bytes) - {}",
                            i,
                            mime_type,
                            data.len(),
                            description
                        );
                    }
                    _ => println!("  [{}] Other metadata chunk", i),
                }
            }

            // Use the original metadata
            original_metadata
        } else {
            println!("No metadata found in source file, using test data");
            vec![
                MetadataChunk::TextTag {
                    key: "TITLE".to_string(),
                    value: "Test Track".to_string(),
                },
                MetadataChunk::TextTag {
                    key: "ARTIST".to_string(),
                    value: "Test Artist".to_string(),
                },
                MetadataChunk::TextTag {
                    key: "ALBUM".to_string(),
                    value: "Test Album".to_string(),
                },
            ]
        };

    // Create minimal audio buffer for testing
    let audio_buffer = AudioBuffer {
        data: vec![vec![0.0f32; 1000]; 2], // 2 channels, 1000 samples each
        channels: 2,
        sample_rate: 44100,
        format: SampleFormat::I16,
    };

    // Test encoding with metadata
    println!("Starting encoding with metadata...");

    // First encode the audio
    let encoded_data = wv_codec.encode(&audio_buffer)?;

    // Then embed metadata into the encoded data
    let encoded_with_metadata = wv_codec.embed_metadata_chunks(&encoded_data, &metadata_chunks)?;

    println!(
        "Encoding completed. Output size: {} bytes",
        encoded_with_metadata.len()
    );

    // Write test output
    let test_output_path = "target/test_metadata_output.wv";
    fs::write(test_output_path, &encoded_with_metadata)?;
    println!("Test file written to: {}", test_output_path);

    println!("Test completed successfully!");

    // Verify the metadata in the output
    println!("\nVerifying metadata in test output...");
    let test_data = fs::read(test_output_path)?;
    if let Ok(metadata_chunks) = wv_codec.extract_metadata_chunks(&test_data) {
        println!(
            "Found {} metadata chunks in test output",
            metadata_chunks.len()
        );
        for (i, chunk) in metadata_chunks.iter().enumerate() {
            match chunk {
                MetadataChunk::TextTag { key, value } => {
                    println!("  [{}] Text tag: {} = {}", i, key, value);
                }
                MetadataChunk::Picture {
                    mime_type,
                    description,
                    data,
                } => {
                    println!(
                        "  [{}] Picture: {} ({} bytes) - {}",
                        i,
                        mime_type,
                        data.len(),
                        description
                    );
                }
                MetadataChunk::Unknown { id, data } => {
                    println!("  [{}] Unknown chunk: {} ({} bytes)", i, id, data.len());
                }
                _ => println!("  [{}] Other metadata chunk", i),
            }
        }
    } else {
        println!("ERROR: No metadata found in test output!");
    }

    // Compare order with original
    println!("\nComparing order between original and output:");
    println!("Original order:");
    for (i, chunk) in metadata_chunks.iter().enumerate() {
        match chunk {
            MetadataChunk::TextTag { key, .. } => {
                println!("  [{}] TextTag: {}", i, key);
            }
            MetadataChunk::Unknown { id, .. } => {
                println!("  [{}] Unknown: {}", i, id);
            }
            _ => println!("  [{}] Other", i),
        }
    }

    let output_chunks = wv_codec.extract_metadata_chunks(&test_data)?;
    println!("Output order:");
    for (i, chunk) in output_chunks.iter().enumerate() {
        match chunk {
            MetadataChunk::TextTag { key, .. } => {
                println!("  [{}] TextTag: {}", i, key);
            }
            MetadataChunk::Unknown { id, .. } => {
                println!("  [{}] Unknown: {}", i, id);
            }
            _ => println!("  [{}] Other", i),
        }
    }

    Ok(())
}
