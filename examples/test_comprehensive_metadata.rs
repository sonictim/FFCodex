use ffcodex_lib::codecs::*;
use ffcodex_lib::*;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing comprehensive WavPack metadata embedding...");

    // Read the source file with metadata
    let source_path = "target/CRWDChld_PlaygroundVocals01_TF_TJFR.wv";
    println!("Reading source file: {}", source_path);

    if !std::path::Path::new(source_path).exists() {
        return Err(format!("Source file not found: {}", source_path).into());
    }

    let source_data = fs::read(source_path)?;
    println!("Source file size: {} bytes", source_data.len());

    // Extract original metadata for comparison
    let wv_codec = WvCodec;
    println!("\nOriginal file metadata:");
    if let Ok(original_metadata) = wv_codec.extract_metadata_chunks(&source_data) {
        println!(
            "Found {} metadata chunks in original file",
            original_metadata.len()
        );
        for chunk in &original_metadata {
            match chunk {
                MetadataChunk::TextTag { key, value } => {
                    println!("  Text tag: {} = {}", key, value);
                }
                MetadataChunk::Picture {
                    mime_type,
                    description,
                    data,
                } => {
                    println!(
                        "  Picture: {} ({} bytes) - {}",
                        mime_type,
                        data.len(),
                        description
                    );
                }
                _ => println!("  Other metadata chunk"),
            }
        }
    }

    // Create minimal audio buffer for testing
    let audio_buffer = AudioBuffer {
        data: vec![vec![0.0f32; 1000]; 2], // 2 channels, 1000 samples each
        channels: 2,
        sample_rate: 44100,
        format: SampleFormat::I16,
    };

    // Create comprehensive metadata matching the original structure
    let metadata_chunks = vec![
        MetadataChunk::TextTag {
            key: "Comment".to_string(),
            value: "Test comment describing the audio content with proper formatting".to_string(),
        },
        MetadataChunk::TextTag {
            key: "Artist".to_string(),
            value: "Test Artist Name".to_string(),
        },
        MetadataChunk::TextTag {
            key: "CatID".to_string(),
            value: "TESTCAT".to_string(),
        },
        MetadataChunk::TextTag {
            key: "SubCategory".to_string(),
            value: "TEST_SUBCATEGORY".to_string(),
        },
        MetadataChunk::TextTag {
            key: "Library".to_string(),
            value: "Test Library Collection".to_string(),
        },
        MetadataChunk::TextTag {
            key: "Title".to_string(),
            value: "Test_Track_Title".to_string(),
        },
        MetadataChunk::TextTag {
            key: "Designer".to_string(),
            value: "Test Designer".to_string(),
        },
        MetadataChunk::TextTag {
            key: "Genre".to_string(),
            value: "TEST_GENRE".to_string(),
        },
        MetadataChunk::TextTag {
            key: "CategoryFull".to_string(),
            value: "TEST_GENRE-TEST_SUBCATEGORY".to_string(),
        },
        MetadataChunk::TextTag {
            key: "FX NAME".to_string(),
            value: "Test FX Effect Name".to_string(),
        },
    ];

    // Test encoding with metadata
    println!("\nStarting encoding with comprehensive metadata...");

    // First encode the audio
    let encoded_data = wv_codec.encode(&audio_buffer)?;

    // Then embed metadata into the encoded data
    let encoded_with_metadata = wv_codec.embed_metadata_chunks(&encoded_data, &metadata_chunks)?;

    println!(
        "Encoding completed. Output size: {} bytes",
        encoded_with_metadata.len()
    );

    // Write test output
    let test_output_path = "target/test_comprehensive_metadata_output.wv";
    fs::write(test_output_path, &encoded_with_metadata)?;
    println!("Test file written to: {}", test_output_path);

    // Verify the metadata in the output
    println!("\nVerifying comprehensive metadata in test output...");
    let test_data = fs::read(test_output_path)?;
    if let Ok(output_metadata) = wv_codec.extract_metadata_chunks(&test_data) {
        println!(
            "Found {} metadata chunks in test output",
            output_metadata.len()
        );

        println!("\nDetailed metadata comparison:");
        for chunk in output_metadata {
            match chunk {
                MetadataChunk::TextTag { key, value } => {
                    println!("  Text tag: '{}' = '{}'", key, value);

                    // Check for formatting issues
                    if value.contains(',') && !key.eq_ignore_ascii_case("comment") {
                        println!("    WARNING: Unexpected comma in value");
                    }
                    if value.is_empty() {
                        println!("    WARNING: Empty value");
                    }
                }
                MetadataChunk::Picture {
                    mime_type,
                    description,
                    data,
                } => {
                    println!(
                        "  Picture: {} ({} bytes) - {}",
                        mime_type,
                        data.len(),
                        description
                    );
                }
                _ => println!("  Other metadata chunk"),
            }
        }
    } else {
        println!("ERROR: No metadata found in test output!");
    }

    println!("\nTest completed successfully!");
    Ok(())
}
