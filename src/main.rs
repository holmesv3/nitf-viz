//! Attempt to read and write thumbnail/gif of image data from a nitf
use log::*;
use clap::Parser;
use simple_logger::SimpleLogger;
use memmap2::Mmap;

use image::{ImageBuffer, DynamicImage, Rgb, Luma, Frame};
use image::imageops::thumbnail;
use image::codecs::gif::{GifEncoder, Repeat};
use nitf_rs::{Nitf, headers::ImageHeader};
use nitf_rs::headers::image_hdr::{ImageRepresentation as IR};


/// Write out the image data from a NITF file. Supported formats:
///      CSI (Color Subapertured Image)
#[derive(Parser, Debug)]
struct Cli {
    /// Input file
    input: std::path::PathBuf,
    
    /// Output folder
    #[arg(short, long, default_value = ".")]
    output: std::path::PathBuf,
    
    /// sqrt(thumbnail size) e.g., -s 50 -> 50^2 pixel image
    /// 
    /// Aspect ratio will be preserved when writing
    #[arg(short, long, default_value="128")]
    size: u32,
    
    /// Whether or not to save the full image
    #[arg(short, long)]
    full: bool,
    
    /// Log level. Choices are off, error, warn, info, debug, or trace
    #[arg(long, default_value = "info")]
    level: LevelFilter,
}

fn get_image(meta: &ImageHeader, raw_data: &Mmap, size: u32) -> DynamicImage {
    let nrows = meta.nrows.val;
    let ncols = meta.ncols.val;
    let nrows = meta.nrows.val;
            
    debug!("Found nrows = {nrows}");
    debug!("Found ncols = {ncols}");
            
    let irep = meta.irep.val;
    debug!("Found irep = {irep}");
    
    let image = match irep {
        IR::RGBLUT => {
            let lut = &meta.bands[0].lutd;
            read_rgb_lut(nrows, ncols, &lut, &raw_data)                 
        },
        IR::RGB => read_rgb(nrows, ncols, &raw_data),
        IR::MONO => read_mono(nrows, ncols, &raw_data),  
         
        _ => todo!("Non-RGB/LUT, RGB, or MONO image representation"),
    };
    // Make thumbnail
    let aspect = ncols as f32 / nrows as f32;
    debug!("Original dimensions: {ncols} X {nrows}");

    let max_size = size.pow(2) as f32;
    let new_width = (aspect * max_size).sqrt() as u32;
    let new_height = (max_size / new_width as f32) as u32;
    debug!("Thumbnail dimensions: {new_width} X {new_height}");
    DynamicImage::from(thumbnail(&image, new_width, new_height))
}


/// Read an mono represented image. Currently assumes all data is a single byte
fn read_mono(nrows: u32, ncols: u32, raw_data: &Mmap) -> DynamicImage {
    let mut image = ImageBuffer::new(ncols, nrows);
    raw_data.iter()
        .zip(image.pixels_mut())
        .for_each(|(data, px)| {
            *px = Luma([*data]);
        });
    DynamicImage::from(image)
}


/// Read an rgb represented image. Currently assumes all data is a single byte
fn read_rgb(nrows: u32, ncols: u32, raw_data: &Mmap) -> DynamicImage {
    let mut image = ImageBuffer::new(ncols, nrows);
    raw_data.chunks(3)
        .zip(image.pixels_mut())
        .for_each(|(data, px)| {
            *px = Rgb([data[0], data[1], data[2]]);
        });
    DynamicImage::from(image)
}


/// Read an rgb_lut represented image. Currently assumes all data is a single byte
fn read_rgb_lut(nrows: u32, ncols: u32, lut: &Vec<Vec<u8>>, raw_data: &Mmap) -> DynamicImage {
    let (r, g, b) = (0, 1, 2);
    let mut image = ImageBuffer::new(ncols, nrows);
    raw_data
        .iter()
        .zip(image.pixels_mut())
        .for_each(|(data, px)| {
            let idx = *data as usize;
            *px = Rgb([lut[r][idx], lut[g][idx], lut[b][idx]]);
        });
    DynamicImage::from(image)
}

fn create_thumbnail(image: &DynamicImage, nrows: u32, ncols: u32, size: u32) -> DynamicImage {
    // Make thumbnail
    let aspect = ncols as f32 / nrows as f32;
    debug!("Original dimensions: {ncols} X {nrows}");

    let max_size = size.pow(2) as f32;
    let new_width = (aspect * max_size).sqrt() as u32;
    let new_height = (max_size / new_width as f32) as u32;
    debug!("Thumbnail dimensions: {new_width} X {new_height}");
    DynamicImage::from(thumbnail(image, new_width, new_height))
}

fn main() {
    
    let args = Cli::parse();
    
    SimpleLogger::new()
        .with_level(args.level)
        .init()
        .unwrap();
    
    let input = std::path::Path::new(&args.input);
    let output = std::path::Path::new(&args.output);
    let out_file;
    let size = args.size;
    
    let _ = match output.try_exists().expect("Don't have permission for that folder") {
        false => std::fs::create_dir_all(output),
        true => Ok(()),
    };
    
    let stem = input.file_stem().unwrap().to_str().unwrap();
    
    debug!("Reading {:}", input.to_str().unwrap());

    let mut nitf_file = std::fs::File::open(input).unwrap();
    let nitf = Nitf::from_reader(&mut nitf_file).unwrap();
    
    let numi = nitf.nitf_header.numi.val;
    debug!("Found numi = {numi}");
    
    // Only dealing with a single image.
    if numi == 1
    {
        let im_seg = &nitf.image_segments[0];
        let meta = &im_seg.header;
        let raw_data = im_seg.get_data_map(&mut nitf_file).unwrap();
        
        out_file = output.join(format!("{stem}_{size}.png"));
        let image = get_image(meta, &raw_data, size);
        
        let _ = image.save(&out_file).unwrap();
        
    }
    else { // num1 > 1
        
        out_file = output.join(format!("{stem}_{size}.gif"));
        let gif_file = std::fs::File::create(&out_file).unwrap();
        
        let mut encoder = GifEncoder::new_with_speed(gif_file, 1);
        let _ = encoder.set_repeat(Repeat::Infinite);
        for (i_frame, seg) in nitf.image_segments.iter().enumerate() {
            let meta = &seg.header;
            let raw_data = seg.get_data_map(&mut nitf_file).unwrap();
                        
            let image = get_image(meta, &raw_data, size);

            info!("Writing frame {} of {numi}", i_frame + 1);
            let frame = Frame::new(image.into());
            let _ = encoder.encode_frame(frame); 
        }
    }
    
    info!("Finished writing {}", out_file.to_str().unwrap());
    
}
