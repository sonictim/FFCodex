use ffcodex_lib::chromaprint_bindings::{Chromaprint, get_version};

fn main() {
    println!("Chromaprint version: {}", get_version());

    // Create a new Chromaprint context
    let chromaprint = Chromaprint::default();

    // Start fingerprinting
    let sample_rate = 44100;
    let num_channels = 2;
    if !chromaprint.start(sample_rate, num_channels) {
        eprintln!("Failed to start Chromaprint");
        return;
    }

    // Create some dummy data
    let dummy_data: Vec<i16> = vec![0; 1024 * 10];
    if !chromaprint.feed(&dummy_data) {
        eprintln!("Failed to feed data to Chromaprint");
        return;
    }

    // Finish fingerprinting
    if !chromaprint.finish() {
        eprintln!("Failed to finish Chromaprint");
        return;
    }

    // Get the fingerprint
    match chromaprint.get_fingerprint() {
        Some(fingerprint) => println!("Fingerprint: {}", fingerprint),
        None => eprintln!("Failed to get fingerprint"),
    }
}