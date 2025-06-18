use ffcodex_lib::codecs::wav::WavCodec;
use ffcodex_lib::prelude::*;

fn main() -> anyhow::Result<()> {
    let codec = WavCodec;

    println!("WAV Codec Description Extraction Test");
    println!("=====================================");

    // Test with a file path (you can replace this with an actual WAV file path)
    let test_file = "/path/to/test.wav";

    println!("Testing WAV file: {}", test_file);

    match codec.get_file_info(test_file) {
        Ok(info) => {
            println!("File Info:");
            println!("  Path: {}", info.path);
            println!("  Size: {} bytes", info.size);
            println!("  Sample Rate: {} Hz", info.sample_rate);
            println!("  Channels: {}", info.channels);
            println!("  Bit Depth: {} bits", info.bit_depth);
            println!("  Duration: {}", info.duration);
            println!("  Description: '{}'", info.description);

            if info.description.is_empty() {
                println!("  Note: No description found in any of the following priority order:");
                println!("    1. bext 'Description' field");
                println!("    2. iXML 'USER_DESCRIPTION'");
                println!("    3. iXML 'BEXT_BWF_DESCRIPTION'");
                println!("    4. ID3 'Comment'");
            } else {
                println!("  Note: Description extracted successfully!");
            }
        }
        Err(e) => {
            println!("Error reading file: {}", e);
            println!("Note: This is expected if the test file doesn't exist.");
        }
    }

    println!("\nDescription Extraction Priority Order:");
    println!("1. bext 'Description' field (first 256 bytes)");
    println!("2. iXML 'USER_DESCRIPTION'");
    println!("3. iXML 'BEXT_BWF_DESCRIPTION'");
    println!("4. ID3 'Comment'");
    println!("5. Empty string (if none found)");

    Ok(())
}
