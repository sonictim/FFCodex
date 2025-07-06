use ffcodex_lib::codecs::FlacCodec;
use ffcodex_lib::prelude::*;
use std::fs;

fn create_test_flac() -> Vec<u8> {
    // Create a minimal valid FLAC file
    let mut data = Vec::new();

    // FLAC marker
    data.extend_from_slice(b"fLaC");

    // STREAMINFO block (last metadata block flag set)
    data.push(0x80); // Last block flag + STREAMINFO type (0)
    data.extend_from_slice(&[0, 0, 34]); // Block size (34 bytes)

    // STREAMINFO data (34 bytes minimum)
    data.extend_from_slice(&[
        0x04, 0x38, // Min block size (1080)
        0x04, 0x38, // Max block size (1080)
        0x00, 0x00, 0x00, // Min frame size (0)
        0x00, 0x00, 0x00, // Max frame size (0)
        0xAC, 0x44, 0x20, // Sample rate (44100), channels (2), bits per sample (16) - packed
        0x00, 0x00, 0x00, 0x00, 0x00, // Total samples (0)
        // MD5 signature (16 bytes)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]);

    // Minimal audio frame (just a header to make it valid)
    // This is a simplified frame header
    data.extend_from_slice(&[
        0xFF, 0xF8, // Frame sync + reserved bits
        0x69, 0x04, // More frame header data
    ]);

    data
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let codec = FlacCodec;

    // Create a test FLAC file
    let original_data = create_test_flac();

    println!("=== Testing FLAC Metadata Round-trip ===");

    // Test 1: Extract metadata from minimal FLAC (should be empty)
    println!("\n1. Testing extraction from minimal FLAC...");
    let extracted_chunks = codec.extract_metadata_chunks(&original_data)?;
    println!(
        "   Extracted {} chunks from minimal FLAC",
        extracted_chunks.len()
    );

    // Test 2: Add various metadata types
    println!("\n2. Testing metadata embedding...");
    let test_chunks = vec![
        MetadataChunk::TextTag {
            key: "TITLE".to_string(),
            value: "Test Song".to_string(),
        },
        MetadataChunk::TextTag {
            key: "ARTIST".to_string(),
            value: "Test Artist".to_string(),
        },
        MetadataChunk::IXml("<BWFXML><TITLE>Test Title</TITLE></BWFXML>".to_string()),
        MetadataChunk::Picture {
            mime_type: "image/jpeg".to_string(),
            description: "Test Picture".to_string(),
            data: vec![0xFF, 0xD8, 0xFF, 0xE0], // JPEG header
        },
    ];

    let modified_data = codec.embed_metadata_chunks(&original_data, &test_chunks)?;
    println!(
        "   Embedded {} chunks, resulting file size: {} bytes",
        test_chunks.len(),
        modified_data.len()
    );

    // Test 3: Extract metadata from modified file
    println!("\n3. Testing extraction from modified FLAC...");
    let extracted_after_embed = codec.extract_metadata_chunks(&modified_data)?;
    println!(
        "   Extracted {} chunks after embedding",
        extracted_after_embed.len()
    );

    // Test 4: Verify the extracted metadata matches what we embedded
    println!("\n4. Verifying round-trip integrity...");
    let mut found_title = false;
    let mut found_artist = false;
    let mut found_ixml = false;
    let mut found_picture = false;

    for chunk in &extracted_after_embed {
        match chunk {
            MetadataChunk::TextTag { key, value } => {
                println!("   Found text tag: {}={}", key, value);
                if key == "TITLE" && value == "Test Song" {
                    found_title = true;
                }
                if key == "ARTIST" && value == "Test Artist" {
                    found_artist = true;
                }
            }
            MetadataChunk::IXml(content) => {
                println!("   Found IXML: {}", content);
                if content.contains("Test Title") {
                    found_ixml = true;
                }
            }
            MetadataChunk::Picture {
                mime_type,
                description,
                data,
            } => {
                println!(
                    "   Found picture: {} bytes, type: {}, desc: {}",
                    data.len(),
                    mime_type,
                    description
                );
                if mime_type == "image/jpeg" && description == "Test Picture" {
                    found_picture = true;
                }
            }
            _ => {
                println!("   Found other chunk: {:?}", chunk);
            }
        }
    }

    // Test 5: Verify file is still valid
    println!("\n5. Validating modified FLAC format...");
    match codec.validate_file_format(&modified_data) {
        Ok(_) => println!("   ✓ Modified FLAC file is valid"),
        Err(e) => println!("   ✗ Modified FLAC file is invalid: {}", e),
    }

    // Test 6: Multiple embedding cycles
    println!("\n6. Testing multiple embedding cycles...");
    let mut current_data = modified_data.clone();
    for i in 1..=3 {
        let cycle_chunks = vec![MetadataChunk::TextTag {
            key: format!("CYCLE{}", i),
            value: format!("Value {}", i),
        }];
        current_data = codec.embed_metadata_chunks(&current_data, &cycle_chunks)?;
        let cycle_extracted = codec.extract_metadata_chunks(&current_data)?;
        println!("   Cycle {}: {} chunks total", i, cycle_extracted.len());
    }

    // Final validation
    match codec.validate_file_format(&current_data) {
        Ok(_) => println!("   ✓ File still valid after multiple cycles"),
        Err(e) => println!("   ✗ File corrupted after cycles: {}", e),
    }

    // Results summary
    println!("\n=== Results Summary ===");
    println!(
        "✓ Title tag: {}",
        if found_title { "FOUND" } else { "MISSING" }
    );
    println!(
        "✓ Artist tag: {}",
        if found_artist { "FOUND" } else { "MISSING" }
    );
    println!(
        "✓ IXML data: {}",
        if found_ixml { "FOUND" } else { "MISSING" }
    );
    println!(
        "✓ Picture data: {}",
        if found_picture { "FOUND" } else { "MISSING" }
    );

    let all_found = found_title && found_artist && found_ixml && found_picture;
    println!(
        "\n{} Round-trip test {}",
        if all_found { "✓" } else { "✗" },
        if all_found { "PASSED" } else { "FAILED" }
    );

    if !all_found {
        return Err("Round-trip test failed - some metadata was lost".into());
    }

    Ok(())
}
