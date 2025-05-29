extern crate ffcodex_lib;
use ffcodex_lib::Codex;
use std::time::Instant;

fn main() {
    // Use the same file as main.rs
    let input_file = "/Users/tfarrell/Desktop/subset test/CRWDChld_PlaygroundVocals01_TF_TJFR.aif";
    let output_file = "/Users/tfarrell/Desktop/subset test/CRWDChld_PlaygroundVocals01_TF_TJFR_resampled.aif";

    println!("=== Profiling AIF Processing ===");
    println!("Input: {}", input_file);
    println!("Output: {}", output_file);
    
    let total_start = Instant::now();

    // Step 1: Load file
    println!("\n--- Step 1: Loading File ---");
    let load_start = Instant::now();
    let mut codex = Codex::new(input_file);
    let load_duration = load_start.elapsed();
    println!("File loading completed in: {:.2}ms", load_duration.as_millis());

    // Print basic file info
    println!("Channels: {}", codex.channels());
    println!("Sample rate: {}Hz", codex.buffer.sample_rate);
    let samples_per_channel = if !codex.buffer.data.is_empty() { codex.buffer.data[0].len() } else { 0 };
    println!("Samples per channel: {}", samples_per_channel);
    let duration_seconds = samples_per_channel as f64 / codex.buffer.sample_rate as f64;
    println!("Duration: {:.2} seconds", duration_seconds);

    // Step 2: Resample
    println!("\n--- Step 2: Resampling ---");
    let resample_start = Instant::now();
    codex.resample(48000);
    let resample_duration = resample_start.elapsed();
    println!("Resampling completed in: {:.2}ms", resample_duration.as_millis());

    // Step 3: Export
    println!("\n--- Step 3: Export ---");
    let export_start = Instant::now();
    match codex.export(output_file) {
        Ok(_) => {
            let export_duration = export_start.elapsed();
            println!("Export completed in: {:.2}ms", export_duration.as_millis());
        }
        Err(e) => {
            eprintln!("Export failed: {}", e);
            std::process::exit(1);
        }
    }

    let total_duration = total_start.elapsed();
    println!("\n=== Summary ===");
    println!("File loading: {:.2}ms ({:.1}%)", 
             load_duration.as_millis(), 
             load_duration.as_millis() as f64 / total_duration.as_millis() as f64 * 100.0);
    println!("Resampling: {:.2}ms ({:.1}%)", 
             resample_duration.as_millis(),
             resample_duration.as_millis() as f64 / total_duration.as_millis() as f64 * 100.0);
    let export_duration = export_start.elapsed();
    println!("Export: {:.2}ms ({:.1}%)", 
             export_duration.as_millis(),
             export_duration.as_millis() as f64 / total_duration.as_millis() as f64 * 100.0);
    println!("Total: {:.2}ms ({:.2}s)", total_duration.as_millis(), total_duration.as_secs_f64());
    
    println!("\nProcessing rate: {:.1}x realtime", 
             duration_seconds / total_duration.as_secs_f64());
}
