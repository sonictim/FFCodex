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

    let fp = get_fingerprint(input_file)?;
    println!("Fingerprint: {}", fp);

    let elapsed_time = start_time.elapsed();
    println!(
        "Finished fingerprinting  in {} seconds",
        elapsed_time.as_secs_f32()
    );

    let output_file = "/Users/tfarrell/Desktop/FLAC output test.flac";

    // // flac_debug(input_file)?;

    let start_time = std::time::Instant::now();

    let mut c = Codex::new(input_file)?.decode()?.extract_metadata()?;

    // c.print_metadata();

    println!(
        "BEFORE: USER_DESIGNER = {:?}",
        c.get_metadata_field("USER_DESIGNER")
    );

    // Test professional metadata workflow
    c.set_metadata_field("USER_DESIGNER", "Jacob Flack")?;
    c.set_metadata_field("USER_DESCRIPTION", "Metal friction sound effects")?;
    c.set_metadata_field("USER_CATEGORY", "METAL")?;
    c.set_metadata_field("USER_SUBCATEGORY", "FRICTION")?;
    c.set_metadata_field("USER_LIBRARY", "TJF Recordings")?;
    c.set_metadata_field("USER_TRACKTITLE", "240628_001")?;
    c.set_metadata_field("USER_MICROPHONE", "Sony PCM-D100")?;
    c.set_metadata_field("USER_CATID", "METLFric")?;
    c.set_metadata_field("USER_LOCATION", "Frisco, TX")?;
    c.set_metadata_field("USER_KEYWORDS", "metal friction squeaks")?;

    println!(
        "AFTER SET: USER_DESIGNER = {:?}",
        c.get_metadata_field("USER_DESIGNER")
    );
    println!(
        "AFTER SET: ASWG_originator = {:?}",
        c.get_metadata_field("ASWG_originator")
    );
    println!(
        "AFTER SET: USER_CATEGORYFULL = {:?}",
        c.get_metadata_field("USER_CATEGORYFULL")
    );

    // c.convert_dual_mono()?;
    println!("Embedding metadata to output file...");
    c.embed_metadata(output_file)?;
    println!("First embedding complete!");
    // clean_multi_mono(input_file)?;

    println!("Reading output file back...");
    let c2 = Codex::new(output_file)?.extract_metadata()?;
    println!("Successfully read output file back!");

    println!(
        "AFTER EMBED: USER_DESIGNER = {:?}",
        c2.get_metadata_field("USER_DESIGNER")
    );
    println!(
        "AFTER EMBED: USER_DESCRIPTION = {:?}",
        c2.get_metadata_field("USER_DESCRIPTION")
    );
    println!(
        "AFTER EMBED: ASWG_originator = {:?}",
        c2.get_metadata_field("ASWG_originator")
    );
    println!(
        "AFTER EMBED: USER_CATEGORYFULL = {:?}",
        c2.get_metadata_field("USER_CATEGORYFULL")
    );
    println!("Re-embedding metadata to output file...");
    c2.embed_metadata(output_file)?;
    println!("Second embedding complete!");

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
