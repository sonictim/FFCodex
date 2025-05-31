use ffcodex_lib::codecs::*;
use ffcodex_lib::*;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== WavPack Metadata Preservation Test ===");
    
    // Test file path
    let test_file = "/Users/tfarrell/Documents/CODE/FFCodex/target/CRWDChld_PlaygroundVocals01_TF_TJFR.wv";
    
    if !Path::new(test_file).exists() {
        println!("Test file not found: {}", test_file);
        println!("Available files in target/:");
        if let Ok(entries) = fs::read_dir("/Users/tfarrell/Documents/CODE/FFCodex/target") {
            for entry in entries {
                if let Ok(entry) = entry {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.ends_with(".wv") {
                            println!("  {}", name);
                        }
                    }
                }
            }
        }
        return Ok(());
    }

    println!("Testing file: {}", test_file);
    
    // Step 1: Load original file and extract metadata
    println!("\n1. Loading original file and extracting metadata...");
    let codex = Codex::new(test_file);
    
    let original_metadata = &codex.metadata;
    println!("Original metadata type: {:?}", 
             match original_metadata {
                 Metadata::Wav(_) => "WAV metadata",
                 _ => "Other metadata type"
             });
    
    let original_chunk_count = match &original_metadata {
        Metadata::Wav(chunks) => {
            println!("Original metadata has {} chunks:", chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                match chunk {
                    MetadataChunk::TextTag { key, value } => {
                        println!("  Chunk {}: TextTag '{}' = '{}'", i, key, value);
                    }
                    MetadataChunk::Picture { mime_type, description, data } => {
                        println!("  Chunk {}: Picture '{}' ({}) - {} bytes", i, description, mime_type, data.len());
                    }
                    MetadataChunk::Bext(_) => {
                        println!("  Chunk {}: BEXT chunk", i);
                    }
                    MetadataChunk::IXml(_) => {
                        println!("  Chunk {}: iXML chunk", i);
                    }
                    _ => {
                        println!("  Chunk {}: Other chunk type", i);
                    }
                }
            }
            chunks.len()
        }
        _ => 0
    };
    
    // Step 2: Check original channel configuration
    println!("\n2. Original audio configuration:");
    println!("   Channels (header): {}", codex.channels());
    println!("   Data channels: {}", codex.data_channels());
    
    // Step 3: Perform dual mono conversion
    println!("\n3. Performing dual mono conversion...");
    let mut codex = codex; // Make it mutable for conversion
    codex.convert_dual_mono()?;
    
    println!("After conversion:");
    println!("   Channels (header): {}", codex.channels());
    println!("   Data channels: {}", codex.data_channels());
    
    // Step 4: Save with metadata preservation
    let output_file = "/tmp/test_output_with_metadata.wv";
    println!("\n4. Saving with metadata to: {}", output_file);
    
    codex.export(output_file)?;
    
    // Step 5: Reload and verify metadata was preserved
    println!("\n5. Reloading file to verify metadata preservation...");
    let test_codex = Codex::new(output_file);
    
    let preserved_chunk_count = match &test_codex.metadata {
        Metadata::Wav(chunks) => {
            println!("Preserved metadata has {} chunks:", chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                match chunk {
                    MetadataChunk::TextTag { key, value } => {
                        println!("  Chunk {}: TextTag '{}' = '{}'", i, key, value);
                    }
                    MetadataChunk::Picture { mime_type, description, data } => {
                        println!("  Chunk {}: Picture '{}' ({}) - {} bytes", i, description, mime_type, data.len());
                    }
                    MetadataChunk::Bext(_) => {
                        println!("  Chunk {}: BEXT chunk", i);
                    }
                    MetadataChunk::IXml(_) => {
                        println!("  Chunk {}: iXML chunk", i);
                    }
                    _ => {
                        println!("  Chunk {}: Other chunk type", i);
                    }
                }
            }
            chunks.len()
        }
        _ => 0
    };
    
    // Step 6: Results
    println!("\n=== RESULTS ===");
    println!("Original chunks: {}", original_chunk_count);
    println!("Preserved chunks: {}", preserved_chunk_count);
    
    if preserved_chunk_count > 0 {
        println!("✅ SUCCESS: Metadata was preserved! ({} chunks)", preserved_chunk_count);
        
        if preserved_chunk_count == original_chunk_count {
            println!("✅ PERFECT: All original metadata chunks were preserved!");
        } else {
            println!("⚠️  PARTIAL: Some metadata chunks were preserved, but count differs");
        }
    } else {
        println!("❌ FAILURE: No metadata was preserved!");
        return Err("Metadata preservation test failed".into());
    }
    
    // Clean up
    let _ = fs::remove_file(output_file);
    
    println!("\n=== Test completed successfully! ===");
    Ok(())
}
