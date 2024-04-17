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
    /// Input file
    pub input: PathBuf,

    /// Output folder
    #[arg(long, default_value = ".")]
    pub output: PathBuf,

    /// sqrt(thumbnail size) e.g., -s 50 -> 50^2 pixel image
    ///
    /// Aspect ratio of input data will be preserved when writing
    #[arg(long, default_value = "512")]
    pub size: u32,

    /// Render multi-segment data as individual images instead of a gif
    #[arg(long, action)]
    pub individual: bool,

    /// Log level. Choices are off, error, warn, info, debug, or trace
    #[arg(long, default_value = "info")]
    pub level: Level,

    /// Enable logging for nitf reading
    #[arg(long, action)]
    pub nitf_log: bool,

    /// Manual brightness adjustment (i32)
    #[arg(long, default_value = "0", allow_hyphen_values = true)]
    pub brightness: i32,

    /// Manual contrast adjustment (f32)
    #[arg(long, default_value = "0", allow_hyphen_values = true)]
    pub contrast: f32,

    /// Use prototype/beta processing
    #[arg(long, action)]
    pub prototype: bool,
}
