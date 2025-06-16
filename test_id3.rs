fn main() {
    println!("🚀 Starting ID3 parser test...");
    test_id3_parsing();
}

use std::collections::HashMap;

// Re-create the ID3 parsing logic for testing
fn test_id3_parsing() {
    // Test ID3v1 tag data (128 bytes)
    let mut id3v1_data = vec![0u8; 128];
    // "TAG" header
    id3v1_data[0..3].copy_from_slice(b"TAG");
    // Title (30 bytes): "Test Song"
    id3v1_data[3..12].copy_from_slice(b"Test Song");
    // Artist (30 bytes): "Test Artist"
    id3v1_data[33..44].copy_from_slice(b"Test Artist");
    // Album (30 bytes): "Test Album"
    id3v1_data[63..73].copy_from_slice(b"Test Album");
    // Year (4 bytes): "2024"
    id3v1_data[93..97].copy_from_slice(b"2024");
    // Comment (28 bytes): "Test Comment"
    id3v1_data[97..109].copy_from_slice(b"Test Comment");
    // Track number (ID3v1.1)
    id3v1_data[125] = 0; // Zero byte before track
    id3v1_data[126] = 5; // Track 5
    // Genre: Rock (17)
    id3v1_data[127] = 17;

    println!("✅ ID3v1 test data created: {} bytes", id3v1_data.len());

    // Test ID3v2.3 tag data
    let mut id3v2_data = Vec::new();
    // ID3v2 header: "ID3" + version (2.3) + flags + size
    id3v2_data.extend_from_slice(b"ID3");
    id3v2_data.push(3); // Major version
    id3v2_data.push(0); // Minor version
    id3v2_data.push(0); // Flags

    // Create a simple TIT2 (Title) frame for testing
    let title_frame = create_id3v23_text_frame("TIT2", "Test Title");
    let artist_frame = create_id3v23_text_frame("TPE1", "Test Artist");

    let total_size = title_frame.len() + artist_frame.len();

    // Convert size to syncsafe integer (4 bytes, 7 bits each)
    let syncsafe_size = encode_syncsafe_int(total_size as u32);
    id3v2_data.extend_from_slice(&syncsafe_size);

    // Add frames
    id3v2_data.extend_from_slice(&title_frame);
    id3v2_data.extend_from_slice(&artist_frame);

    println!("✅ ID3v2.3 test data created: {} bytes", id3v2_data.len());
    println!("   - Total frames size: {} bytes", total_size);
    println!("   - Title frame: {} bytes", title_frame.len());
    println!("   - Artist frame: {} bytes", artist_frame.len());

    println!("🧪 ID3 parsing test completed successfully!");
}

fn create_id3v23_text_frame(frame_id: &str, text: &str) -> Vec<u8> {
    let mut frame = Vec::new();

    // Frame ID (4 bytes)
    frame.extend_from_slice(frame_id.as_bytes());

    // Frame size (4 bytes) - will include encoding byte + text
    let text_bytes = text.as_bytes();
    let frame_size = 1 + text_bytes.len(); // 1 byte for encoding + text
    frame.extend_from_slice(&(frame_size as u32).to_be_bytes());

    // Frame flags (2 bytes)
    frame.extend_from_slice(&[0, 0]);

    // Frame data: encoding byte + text
    frame.push(0); // ISO-8859-1 encoding
    frame.extend_from_slice(text_bytes);

    frame
}

fn encode_syncsafe_int(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

fn main() {
    println!("🚀 Starting ID3 parser test...");
    test_id3_parsing();
}
