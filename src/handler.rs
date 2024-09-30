//! Definition of image reading/writing logic
use std::fs::File;

use quick_xml::events::Event;

use log::debug;

use nitf_rs::headers::image_hdr::ImageRepresentation;
use nitf_rs::Nitf;

use crate::cli::Cli;
use crate::mono::make_mono;
use crate::rgb::make_rgb;
use crate::rgb_lut::make_rgb_lut;
use crate::sicd::make_sicd;
use crate::{VizError, VizResult};

/// Specialized inputs
#[derive(Default, Debug)]
pub enum InputType {
    Sicd,
    #[default]
    Other,
}

/// Top level handler for all program logic
pub struct Handler {
    /// Input file name
    pub stem: String,
    /// Input file path
    pub input: std::path::PathBuf,
    /// Output directory
    pub out_dir: std::path::PathBuf,
    /// Output image(s) size
    pub size: u32,
}

/// Takes care of all reading, parsing, and writing work
impl Handler {
    pub fn run(input: &std::path::Path, args: &Cli) -> VizResult<String> {
        let stem = input
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap()
            .to_string();

        let size = args.size;
        let out_dir = args.output.clone();

        let _ = match out_dir
            .try_exists()
            .expect("Don't have permission for that folder")
        {
            false => std::fs::create_dir_all(&out_dir),
            true => Ok(()),
        };

        debug!("Reading {:}", input.to_str().unwrap());
        let mut nitf_file = File::open(input)?;
        let nitf = Nitf::from_reader(&mut nitf_file)?;
        let im_hdr = &nitf.image_segments[0].header;

        // Check for 'special' input type
        let pix_type = im_hdr.irep.val;

        // Check for XML, right now there is only special logic for the SICD
        let input_type = if nitf.nitf_header.numdes.val > 0 {
            let xml = nitf.data_extension_segments[0].get_data_map(&mut nitf_file)?;
            let mut reader = quick_xml::Reader::from_str(std::str::from_utf8(&xml[..])?);
            let event = reader.read_event();
            match event {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    let tag_str = std::str::from_utf8(name.as_ref()).unwrap();
                    if tag_str.contains("SICD") {
                        InputType::Sicd
                    } else {
                        InputType::Other
                    }
                }
                _ => InputType::Other, // no-op
            }
        } else {
            InputType::Other
        }; // no xml/des

        let obj = Self {
            stem,
            out_dir,
            size,
            input: input.to_path_buf(),
        };
        // Only dealing with a single image for now.
        match input_type {
            InputType::Sicd => make_sicd(obj),
            _ => match pix_type {
                ImageRepresentation::MONO => make_mono(obj),
                ImageRepresentation::RGB => make_rgb(obj),
                ImageRepresentation::RGBLUT => make_rgb_lut(obj),
                _ => Err(VizError::Unimplemented),
            },
        }
    }

    pub fn calc_size(&self, n_rows: f32, n_cols: f32) -> (u32, u32) {
        let aspect = n_cols / n_rows;

        let max_size = if self.size != 0 {
            self.size.pow(2) as f32
        } else {
            n_rows * n_cols
        };
        let out_cols = (aspect * max_size).sqrt() as u32;
        let out_rows = (max_size / out_cols as f32) as u32;
        debug!("Creating image with {} x {} pixels", out_cols, out_rows);
        (out_rows, out_cols)
    }
}
