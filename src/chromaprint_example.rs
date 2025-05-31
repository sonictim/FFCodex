mod chromaprint_bindings;

use chromaprint_bindings::{Chromaprint, get_version};

fn main() {
    dprintln!("Chromaprint version: {}", get_version());

    let chromaprint = Chromaprint::default();

    // Start fingerprinting
    let sample_rate = 44100;
    let num_channels = 2;
    if !chromaprint.start(sample_rate, num_channels) {
        edprintln!("Failed to start Chromaprint");
        return;
    }

    // Here you would load audio data and feed it to Chromaprint
    // For example:
    // let audio_data: Vec<i16> = load_audio_file("audio.wav");
    // chromaprint.feed(&audio_data);

    // For this example, we'll just create some dummy data
    let dummy_data: Vec<i16> = vec![0; 1024 * 10];
    if !chromaprint.feed(&dummy_data) {
        edprintln!("Failed to feed data to Chromaprint");
        return;
    }

    // Finish fingerprinting
    if !chromaprint.finish() {
        edprintln!("Failed to finish Chromaprint");
        return;
    }

    // Get the fingerprint
    match chromaprint.get_fingerprint() {
        Some(fingerprint) => dprintln!("Fingerprint: {}", fingerprint),
        None => edprintln!("Failed to get fingerprint"),
    }
}
