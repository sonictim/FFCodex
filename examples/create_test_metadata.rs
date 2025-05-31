use ffcodex_lib::codecs::*;
use ffcodex_lib::*;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Creating Test WavPack File with Metadata ===");
    
    // Use one of the existing WavPack files as a base
    let source_file = "/Users/tfarrell/Documents/CODE/FFCodex/target/CRWDChld_PlaygroundVocals01_TF_TJFR.wv";
    let test_file_with_metadata = "/tmp/test_with_metadata.wv";
    
    // Copy the source file
    fs::copy(source_file, test_file_with_metadata)?;
    
    // Create test metadata
    let mut test_chunks = Vec::new();
    
    // Add some text tags
    test_chunks.push(MetadataChunk::TextTag {
        key: "TITLE".to_string(),
        value: "Test Song Title".to_string(),
    });
    
    test_chunks.push(MetadataChunk::TextTag {
        key: "ARTIST".to_string(),
        value: "Test Artist Name".to_string(),
    });
    
    test_chunks.push(MetadataChunk::TextTag {
        key: "ALBUM".to_string(),
        value: "Test Album Name".to_string(),
    });
    
    test_chunks.push(MetadataChunk::TextTag {
        key: "GENRE".to_string(),
        value: "Electronic".to_string(),
    });
    
    // Create iXML metadata
    let ixml_content = r#"<BWF_iXML_1.0>
<PROJECT>Test Project</PROJECT>
<SCENE>Scene 01</SCENE>
<TAPE>Tape A</TAPE>
<CHANNELS>2</CHANNELS>
<SAMPLE_RATE>48000</SAMPLE_RATE>
</BWF_iXML_1.0>"#;
    
    test_chunks.push(MetadataChunk::IXml(ixml_content.to_string()));
    
    let test_metadata_chunks = test_chunks;
    
    // Load the source file and embed metadata using the codec directly
    let source_data = fs::read(test_file_with_metadata)?;
    let wv_codec = WvCodec;
    
    // Embed the metadata into the file
    let data_with_metadata = wv_codec.embed_metadata_chunks(&source_data, &test_metadata_chunks)?;
    fs::write(test_file_with_metadata, data_with_metadata)?;
    
    println!("✅ Created test file with metadata: {}", test_file_with_metadata);
    
    // Verify the metadata was embedded
    let verify_data = fs::read(test_file_with_metadata)?;
    let extracted_chunks = wv_codec.extract_metadata_chunks(&verify_data)?;
    
    println!("✅ Verified: Test file contains {} metadata chunks:", extracted_chunks.len());
    for (i, chunk) in extracted_chunks.iter().enumerate() {
        match chunk {
            MetadataChunk::TextTag { key, value } => {
                println!("  Chunk {}: {} = {}", i, key, value);
            }
            MetadataChunk::IXml(_) => {
                println!("  Chunk {}: iXML chunk", i);
            }
            _ => {
                println!("  Chunk {}: Other type", i);
            }
        }
    }
    
    println!("\nTest file created: {}", test_file_with_metadata);
    println!("You can now run the metadata preservation test with this file.");
    
    Ok(())
}
