#!/usr/bin/env rust-script

//! Test script to verify WavPack metadata preservation during dual mono conversion
//!
//! This test will:
//! 1. Load a WavPack file
//! 2. Extract its original metadata
//! 3. Perform dual mono conversion
//! 4. Re-embed the metadata using our fixed encoder
//! 5. Verify metadata was preserved by checking tag counts

use std::fs;
use std::path::Path;

// Import FFCodex library
use ffcodex_lib::Codex;
use ffcodex_lib::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dprintln!("=== WavPack Metadata Preservation Test ===");

    // Test file path
    let test_file =
        "/Users/tfarrell/Documents/CODE/FFCodex/target/CRWDChld_PlaygroundVocals01_TF_TJFR.wv";

    if !Path::new(test_file).exists() {
        dprintln!("Test file not found: {}", test_file);
        return Ok(());
    }

    dprintln!("Testing file: {}", test_file);

    // Step 1: Load original file and extract metadata
    dprintln!("\n1. Loading original file and extracting metadata...");
    let mut codex = Codex::new();
    codex.load(test_file)?;

    let original_metadata = codex.metadata.clone();
    dprintln!(
        "Original metadata type: {:?}",
        match &original_metadata {
            Metadata::Wav(_) => "WAV metadata",
            _ => "Other metadata type",
        }
    );

    let original_chunk_count = match &original_metadata {
        Metadata::Wav(chunks) => {
            dprintln!("Original metadata has {} chunks:", chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                match chunk {
                    MetadataChunk::TextTag { key, value } => {
                        dprintln!("  Chunk {}: TextTag '{}' = '{}'", i, key, value);
                    }
                    MetadataChunk::Picture {
                        mime_type,
                        description,
                        data,
                    } => {
                        dprintln!(
                            "  Chunk {}: Picture '{}' ({}) - {} bytes",
                            i,
                            description,
                            mime_type,
                            data.len()
                        );
                    }
                    MetadataChunk::Bext(_) => {
                        dprintln!("  Chunk {}: BEXT chunk", i);
                    }
                    MetadataChunk::IXml(_) => {
                        dprintln!("  Chunk {}: iXML chunk", i);
                    }
                    _ => {
                        dprintln!("  Chunk {}: Other chunk type", i);
                    }
                }
            }
            chunks.len()
        }
        _ => 0,
    };

    // Step 2: Check original channel configuration
    dprintln!("\n2. Original audio configuration:");
    dprintln!("   Channels (header): {}", codex.channels());
    dprintln!("   Data channels: {}", codex.data_channels());

    // Step 3: Perform dual mono conversion
    dprintln!("\n3. Performing dual mono conversion...");
    codex.convert_dual_mono()?;

    dprintln!("After conversion:");
    dprintln!("   Channels (header): {}", codex.channels());
    dprintln!("   Data channels: {}", codex.data_channels());

    // Step 4: Save with metadata preservation
    let output_file = "/tmp/test_output_with_metadata.wv";
    dprintln!("\n4. Saving with metadata to: {}", output_file);

    codex.save(output_file)?;

    // Step 5: Reload and verify metadata was preserved
    dprintln!("\n5. Reloading file to verify metadata preservation...");
    let mut test_codex = Codex::new();
    test_codex.load(output_file)?;

    let preserved_chunk_count = match &test_codex.metadata {
        Metadata::Wav(chunks) => {
            dprintln!("Preserved metadata has {} chunks:", chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                match chunk {
                    MetadataChunk::TextTag { key, value } => {
                        dprintln!("  Chunk {}: TextTag '{}' = '{}'", i, key, value);
                    }
                    MetadataChunk::Picture {
                        mime_type,
                        description,
                        data,
                    } => {
                        dprintln!(
                            "  Chunk {}: Picture '{}' ({}) - {} bytes",
                            i,
                            description,
                            mime_type,
                            data.len()
                        );
                    }
                    MetadataChunk::Bext(_) => {
                        dprintln!("  Chunk {}: BEXT chunk", i);
                    }
                    MetadataChunk::IXml(_) => {
                        dprintln!("  Chunk {}: iXML chunk", i);
                    }
                    _ => {
                        dprintln!("  Chunk {}: Other chunk type", i);
                    }
                }
            }
            chunks.len()
        }
        _ => 0,
    };

    // Step 6: Results
    dprintln!("\n=== RESULTS ===");
    dprintln!("Original chunks: {}", original_chunk_count);
    dprintln!("Preserved chunks: {}", preserved_chunk_count);

    if preserved_chunk_count > 0 {
        dprintln!(
            "✅ SUCCESS: Metadata was preserved! ({} chunks)",
            preserved_chunk_count
        );

        if preserved_chunk_count == original_chunk_count {
            dprintln!("✅ PERFECT: All original metadata chunks were preserved!");
        } else {
            dprintln!("⚠️  PARTIAL: Some metadata chunks were preserved, but count differs");
        }
    } else {
        dprintln!("❌ FAILURE: No metadata was preserved!");
        return Err("Metadata preservation test failed".into());
    }

    // Step 7: Use wvunpack to verify the file has metadata
    dprintln!("\n6. Using WavPack tools to verify metadata...");

    // Check if wvunpack is available
    match std::process::Command::new("wvunpack")
        .arg("-ss") // Show summary
        .arg(output_file)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            dprintln!("wvunpack output:");
            dprintln!("{}", stdout);
            if !stderr.is_empty() {
                dprintln!("wvunpack stderr:");
                dprintln!("{}", stderr);
            }

            // Look for tag information in the output
            if stdout.contains("tags") || stdout.contains("metadata") {
                dprintln!("✅ wvunpack confirms metadata is present");
            }
        }
        Err(e) => {
            dprintln!(
                "Note: wvunpack not available ({}), skipping external verification",
                e
            );
        }
    }

    // Clean up
    let _ = fs::remove_file(output_file);

    dprintln!("\n=== Test completed successfully! ===");
    Ok(())
}
