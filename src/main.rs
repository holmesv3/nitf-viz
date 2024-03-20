use clap::Parser;
use log::LevelFilter;

/// Fill me out eventually
#[derive(Parser)]
struct Cli {
    /// Input file to read
    input: std::path::PathBuf,
    /// Ouput fold to write images
    #[arg(short, long, default_value = "./")]
    output: std::path::PathBuf,
    /// Log level
    #[arg(long, default_value = "warn")]
    level: LevelFilter,
}

fn main() {
    let args = Cli::parse();
    simple_logger::SimpleLogger::new()
        .with_level(args.level)
        .init()
        .unwrap();

    // Create the output path
    match args.output
        .try_exists()
        .expect("No read access to output directory")
    {
        true => Ok(()),
        false => std::fs::create_dir_all(&args.output),
    }
    .expect("Could not create output directory");

    let stem = args.input.file_stem().unwrap().to_str().unwrap();
    
}
