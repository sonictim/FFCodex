use ffcodex_lib::*;
use ffcodex_lib::codecs::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_file = "/Users/tfarrell/Desktop/subset test/THND_Fstorm_LUD018.159_shorter.wv";
    let output_file = "/Users/tfarrell/Desktop/subset test/THND_Fstorm_LUD018.159_shorter_test.wv";
    
    println!("=== Testing metadata preservation ===");
    
    // Test 1: Check if input file has metadata
    println!("1. Checking input file metadata...");
    let codec = get_codec(input_file)?;
    let metadata = codec.extract_metadata_from_file(input_file)?;
    
    match &metadata {
        Metadata::Wav(chunks) => {
            println!("Found {} metadata chunks in input file:", chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                match chunk {
                    MetadataChunk::TextTag { key, value } => {
                        println!("  [{}] Text: {} = {}", i, key, value);
                    }
                    MetadataChunk::Bext(_) => {
                        println!("  [{}] BEXT chunk", i);
                    }
                    MetadataChunk::IXml(_) => {
                        println!("  [{}] iXML chunk", i);
                    }
                    _ => {
                        println!("  [{}] Other: {}", i, chunk.id());
                    }
                }
            }
        }
        _ => println!("No WAV metadata found"),
    }
    
    // Test 2: Use Codex to process the file
    println!("\n2. Using Codex to process file...");
    let mut codex = Codex::new(input_file);
    
    println!("Codex metadata has:");
    match &codex.metadata {
        Metadata::Wav(chunks) => {
            println!("  {} chunks loaded into Codex", chunks.len());
        }
        _ => println!("  No metadata in Codex"),
    }
    
    // Convert dual mono (this modifies the audio buffer)
    codex.convert_dual_mono()?;
    
    // Export the file
    println!("\n3. Exporting file...");
    codex.export(output_file)?;
    
    // Test 3: Check output file metadata
    println!("\n4. Checking output file metadata...");
    let output_metadata = codec.extract_metadata_from_file(output_file)?;
    
    match &output_metadata {
        Metadata::Wav(chunks) => {
            println!("Found {} metadata chunks in output file:", chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                match chunk {
                    MetadataChunk::TextTag { key, value } => {
                        println!("  [{}] Text: {} = {}", i, key, value);
                    }
                    MetadataChunk::Bext(_) => {
                        println!("  [{}] BEXT chunk", i);
                    }
                    MetadataChunk::IXml(_) => {
                        println!("  [{}] iXML chunk", i);
                    }
                    _ => {
                        println!("  [{}] Other: {}", i, chunk.id());
                    }
                }
            }
        }
        _ => println!("No WAV metadata found in output"),
    }
    
    println!("\n=== Test complete ===");
    
    Ok(())
}
