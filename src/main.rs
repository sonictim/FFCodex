pub use anyhow::{Result as R, anyhow};
use ffcodex_lib::*;

fn main() -> R<()> {
    // Use the directly exported get_version function
    let version = chromaprint_bindings::get_version();
    let start_time = std::time::Instant::now();
    println!("Chromaprint version: {}", version);

    // Get input file from command line arguments
    let _args: Vec<String> = std::env::args().collect();
    // if args.len() < 2 {
    //     eprintln!("Usage: {} <input_file>", args[0]);
    //     std::process::exit(1);
    // }

    // let input_file = if args.len() > 1 && !args[1].is_empty() {
    //     &args[1]
    // } else {
    //     "/Users/tfarrell/Desktop/subset test/CRWDChld_PlaygroundVocals01_TF_TJFR.aif"
    // };

    // let input_file = "/Users/tfarrell/Desktop/DUAL MONO IDEAS/Cloth-Blanket-Vinyl-Backing-Movement_GEN-HD2-28950MOD.flac";
    let input_file = "/Users/tfarrell/Desktop/FOODEat_TempVeggieFlac_TF_TJFR copy.flac";

    // clean_multi_mono(input_file)?;

    println!("Input file: {}", input_file);
    let data = get_basic_metadata(input_file)?;
    println!("Basic metadata: {:?}", data);

    // let fp = get_fingerprint(input_file)?;
    // println!("Fingerprint: {}", fp);

    // let elapsed_time = start_time.elapsed();
    // println!(
    //     "Finished fingerprinting  in {} seconds",
    //     elapsed_time.as_secs_f32()
    // );

    let output_file = "/Users/tfarrell/Desktop/FLAC output test.flac";

    // // flac_debug(input_file)?;

    // let start_time = std::time::Instant::now();

    let mut c = Codex::new(input_file)?.extract_metadata()?;

    c.parse_metadata()?;
    println!("{:?}", c.get_metadata_field("USER_DESIGNER"));
    c.set_metadata_field("USER_DESIGNER", "Jacob Flack")?;
    println!("{:?}", c.get_metadata_field("USER_DESIGNER"));
    c.remove_soundminer_metadata_chunk()?;
    // c.convert_dual_mono()?;
    c.embed_metadata(output_file)?;
    // clean_multi_mono(input_file)?;

    let mut c2 = Codex::new(output_file)?.extract_metadata()?;

    c2.parse_metadata()?;
    println!("{:?}", c2.get_metadata_field("DESCRIPTION"));
    c2.set_metadata_field("DESCRIPTION", "Two junkings together")?;
    println!("{:?}", c2.get_metadata_field("DESCRIPTION"));
    c2.embed_metadata(output_file)?;

    let elapsed_time = start_time.elapsed();
    println!("Finished in {} seconds", elapsed_time.as_secs_f32());

    // let _c = Codex::new(output_file);

    // flac_debug(output_file)?;

    Ok(())
}

// fn flac_debug(path: &str) -> R<()> {
//     let input = std::fs::read(path)?;
//     println!(
//         "FLAC DEBUG: Input first bytes: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
//         input[0], input[1], input[2], input[3], input[4], input[5], input[6], input[7]
//     );

//     Ok(())
// }
