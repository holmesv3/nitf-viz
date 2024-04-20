use log::LevelFilter;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Level {
    /// A level lower than all log levels.
    Off,
    /// Corresponds to the `Error` log level.
    Error,
    /// Corresponds to the `Warn` log level.
    Warn,
    /// Corresponds to the `Info` log level.
    Info,
    /// Corresponds to the `Debug` log level.
    Debug,
    /// Corresponds to the `Trace` log level.
    Trace,
}

impl From<Level> for LevelFilter {
    fn from(value: Level) -> Self {
        match value {
            Level::Off => LevelFilter::Off,
            Level::Error => LevelFilter::Error,
            Level::Warn => LevelFilter::Warn,
            Level::Info => LevelFilter::Info,
            Level::Debug => LevelFilter::Debug,
            Level::Trace => LevelFilter::Trace,
        }
    }
}
/// Write out the image data from a NITF file.
#[derive(Parser, Debug)]
pub struct Cli {
    /// Input NITF file
    pub input: PathBuf,

    /// Output folder
    #[arg(long, default_value = ".")]
    pub output: PathBuf,

    /// Output file name. Derived from input if not given.
    #[arg(short, long)]
    pub prefix: Option<String>,

    /// sqrt(num-pixels) e.g., --size 50 -> 50^2 pixel image
    ///
    /// Aspect ratio of input data will be preserved when writing
    #[arg(short, long, default_value = "256")]
    pub size: u32,

    /// Adjust the brightness of the image product (32-bit signed integer)
    #[arg(short, long, default_value = "0", allow_hyphen_values = true)]
    pub brightness: i32,

    /// Adjust the contrast of the image product (32-bit float)
    #[arg(short, long, default_value = "0", allow_hyphen_values = true)]
    pub contrast: f32,

    /// Log level
    #[arg(long, default_value = "info")]
    pub level: Level,

    /// Enable logging for nitf reading
    #[arg(long, action)]
    pub nitf_log: bool,
}
