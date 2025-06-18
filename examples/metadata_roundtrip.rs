use ffcodex_lib::codecs::MetadataChunk;
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Create a simple text metadata chunk
    let mut text_metadata = HashMap::new();
    text_metadata.insert("title".to_string(), "My Song".to_string());

    let text_chunk = MetadataChunk::from_hashmap(&text_metadata, "text")?;
    println!("Created text chunk: {:?}", text_chunk);

    // Parse it back
    let parsed = text_chunk.parse()?;
    println!("Parsed back: {:?}", parsed);

    // Example 2: Create a BEXT metadata chunk
    let mut bext_metadata = HashMap::new();
    bext_metadata.insert(
        "Description".to_string(),
        "Audio recording for project X".to_string(),
    );
    bext_metadata.insert("Originator".to_string(), "My Studio".to_string());
    bext_metadata.insert("OriginatorReference".to_string(), "REF001".to_string());
    bext_metadata.insert("OriginationDate".to_string(), "2025-06-16".to_string());
    bext_metadata.insert("OriginationTime".to_string(), "14:30:00".to_string());
    bext_metadata.insert("TimeReference".to_string(), "0".to_string());
    bext_metadata.insert("Version".to_string(), "1".to_string());

    let bext_chunk = MetadataChunk::from_hashmap(&bext_metadata, "bext")?;
    println!("\nCreated BEXT chunk: ID = {}", bext_chunk.id());

    // Parse it back
    let parsed_bext = bext_chunk.parse()?;
    println!("Parsed BEXT back: {:?}", parsed_bext);

    // Example 3: Create an iXML metadata chunk
    let mut ixml_metadata = HashMap::new();
    ixml_metadata.insert("PROJECT".to_string(), "My Project".to_string());
    ixml_metadata.insert("SCENE".to_string(), "Scene 1".to_string());
    ixml_metadata.insert("TAKE".to_string(), "Take 3".to_string());
    ixml_metadata.insert("SPEED_TAPE_SPEED".to_string(), "25".to_string());

    let ixml_chunk = MetadataChunk::from_hashmap(&ixml_metadata, "ixml")?;
    println!("\nCreated iXML chunk: ID = {}", ixml_chunk.id());

    // Parse it back
    let parsed_ixml = ixml_chunk.parse()?;
    println!("Parsed iXML back: {:?}", parsed_ixml);

    // Example 4: Create an ID3 metadata chunk
    let mut id3_metadata = HashMap::new();
    id3_metadata.insert("Title".to_string(), "My Song".to_string());
    id3_metadata.insert("Artist".to_string(), "My Band".to_string());
    id3_metadata.insert("Album".to_string(), "My Album".to_string());
    id3_metadata.insert("Year".to_string(), "2025".to_string());
    id3_metadata.insert("Genre".to_string(), "Rock".to_string());

    let id3_chunk = MetadataChunk::from_hashmap(&id3_metadata, "id3")?;
    println!("\nCreated ID3 chunk: ID = {}", id3_chunk.id());

    // Parse it back
    let parsed_id3 = id3_chunk.parse()?;
    println!("Parsed ID3 back: {:?}", parsed_id3);

    Ok(())
}
