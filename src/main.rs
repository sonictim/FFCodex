pub use anyhow::{Result as R, anyhow};
use ffcodex_lib::*;

fn main() -> R<()> {
    // let input_file = "/Users/tfarrell/Desktop/subset test/THUNDER DUAL MONO 2.wav";
    // let output_file = "/Users/tfarrell/Desktop/subset test/THUNDER DUAL MONO 2.wav";

    // flac_debug(input_file)?;

    let start_time = std::time::Instant::now();

    clean_multi_mono("/Users/tfarrell/Desktop/subset test/THUNDER DUAL MONO 2.wav")?;

    // let mut c = Codex::new(input_file);
    // c.convert_dual_mono()?;
    // c.export(output_file)?;

    let elapsed_time = start_time.elapsed();
    println!("Finished in {} seconds", elapsed_time.as_secs_f32());

    // flac_debug(output_file)?;

    Ok(())
}

fn flac_debug(path: &str) -> R<()> {
    let input = std::fs::read(path)?;
    println!(
        "FLAC DEBUG: Input first bytes: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
        input[0], input[1], input[2], input[3], input[4], input[5], input[6], input[7]
    );

    Ok(())
}
