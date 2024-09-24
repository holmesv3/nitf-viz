//! Definition of image reading/writing logic
use std::fs::File;

use quick_xml::events::Event;

use log::debug;

use nitf_rs::headers::image_hdr::ImageRepresentation;
use nitf_rs::Nitf;

use crate::cli::Cli;
use crate::sicd::make_sicd;
use crate::sidd::make_sidd;
use crate::VizResult;

/// Specialized inputs
pub enum InputType {
    SICD,
    SIDD,
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
    /// Input type
    pub input_type: InputType,
    /// Pixel type
    pub pix_type: ImageRepresentation,
}

/// Takes care of all reading, parsing, and writing work
impl Handler {
    pub fn run(input: &std::path::PathBuf, args: &Cli) -> VizResult<String> {
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
        let mut nitf_file = File::open(input.clone())?;
        let nitf = Nitf::from_reader(&mut nitf_file)?;
        let im_hdr = &nitf.image_segments[0].header;

        // Determine input type WIP
        let mut input_type = InputType::Other;

        // Check for XML
        if nitf.nitf_header.numdes.val > 0 {
            let xml = nitf.data_extension_segments[0].get_data_map(&mut nitf_file)?;
            let mut reader = quick_xml::Reader::from_str(std::str::from_utf8(&xml[..100])?);
            input_type = reader.read_event().map(|event| match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"SICD" => InputType::SICD,
                    b"SIDD" => InputType::SIDD,
                    _ => InputType::Other,
                },
                _ => InputType::Other,
            })?;
        }

        let obj = Self {
            stem,
            out_dir,
            size,
            input: input.clone(),
            input_type,
            pix_type: im_hdr.irep.val,
        };
        // Only dealing with a single image for now.
        match &obj.input_type {
            InputType::SICD => make_sicd(obj),
            InputType::SIDD => make_sidd(obj),
            _ => todo!(),
        }
    }

    // pub fn multi_segment(&self, stem: String) -> VizResult<String> {
    //     let out_file = self.out_dir.join(format!("{stem}.gif"));
    //     let gif_file = File::create(&out_file)?;

    //     let mut encoder = GifEncoder::new_with_speed(gif_file, 1);
    //     let _ = encoder.set_repeat(Repeat::Infinite);
    //     for i_seg in 0..self.numi {
    //         let image = self.get_image(i_seg.into())?;
    //         info!("Writing frame {} of {}", i_seg + 1, self.numi);
    //         let frame = Frame::new(image);
    //         let _ = encoder.encode_frame(frame);
    //     }
    //     Ok(out_file.to_str().unwrap().to_string())
    // }
}
