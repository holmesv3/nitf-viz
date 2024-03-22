use nitf_rs::{Nitf, ImageSegment};
use nitf_rs::headers::image_hdr::{
    PixelValueType, 
    ImageRepresentation,
    ImageRepresentationBand,
    Band, 
    
};
use image::{ImageBuffer, Rgb};


fn main() {
    let dim = 2_u32.pow(8);
    
    let mut rgb = ImageBuffer::new(dim, dim);
    for row in 0..(dim / 2) {
        for col in (dim / 2)..dim {
            rgb.put_pixel(row, col, Rgb([u8::MAX, u8::MAX / 2, 0]));
        }
    }
    for row in (dim / 2)..dim {
        for col in 0..(dim / 2) {
            rgb.put_pixel(row, col, Rgb([0, u8::MAX / 2, u8::MAX]));
        }
    }
    let mut rgb_nitf = Nitf::default();
    let mut rgb_segment = ImageSegment {
        data_size: 3 * dim.pow(2) as u64,
        ..Default::default()
    };
    let rgb_header = &mut rgb_segment.header;
    rgb_header.nrows.val = dim;
    rgb_header.ncols.val = dim;
    rgb_header.pvtype.val = PixelValueType::INT;
    rgb_header.irep.val = ImageRepresentation::RGB;
    rgb_header.nbpp.val = 24;
    rgb_header.abpp.val = 24;
    rgb_header.nbands.val = 3;
    rgb_header.icat.val = "VIS".to_string();
    let mut red_band = Band::default();
    red_band.irepband.val = ImageRepresentationBand::R;
    let mut green_band = Band::default();
    green_band.irepband.val = ImageRepresentationBand::G;
    let mut blue_band = Band::default();
    blue_band.irepband.val = ImageRepresentationBand::B;
    rgb_header.bands = vec![red_band, green_band, blue_band];
    
    rgb_nitf.add_im(rgb_segment);
    let mut rgb_file = std::fs::File::create("examples/rgb.nitf").unwrap();
    rgb_nitf.write_headers(&mut rgb_file).unwrap();
    rgb_nitf.image_segments[0].write_data(&mut rgb_file, &rgb.into_raw()).unwrap();
}