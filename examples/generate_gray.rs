use image::{ImageBuffer, Luma};
use nitf_rs::headers::image_hdr::{
    Band, ImageRepresentation, ImageRepresentationBand, Mode, PixelValueType,
};
use nitf_rs::{ImageSegment, Nitf};

fn main() {
    // Make a grayscale image
    let dim = 2_u32.pow(8);
    let mut gray = ImageBuffer::new(dim, dim);
    for row in 0..(dim / 2) {
        for col in (dim / 2)..dim {
            gray.put_pixel(row, col, Luma([u8::MAX]));
        }
    }
    for row in (dim / 2)..dim {
        for col in 0..(dim / 2) {
            gray.put_pixel(row, col, Luma([u8::MAX]));
        }
    }
    let mut gray_nitf = Nitf::default();
    let mut gray_segment = ImageSegment {
        data_size: dim.pow(2) as u64,
        ..Default::default()
    };
    let gray_header = &mut gray_segment.header;
    gray_header.nrows.val = dim;
    gray_header.ncols.val = dim;
    gray_header.pvtype.val = PixelValueType::INT;
    gray_header.irep.val = ImageRepresentation::MONO;
    gray_header.nbpp.val = 8;
    gray_header.abpp.val = 8;
    gray_header.nbands.val = 1;
    gray_header.imode.val = Mode::B;
    gray_header.icat.val = "VIS".to_string();
    let mut gray_band = Band::default();
    gray_band.irepband.val = ImageRepresentationBand::M;
    gray_header.bands = vec![gray_band];

    gray_nitf.add_im(gray_segment);
    let mut gray_file = std::fs::File::create("examples/gray.nitf").unwrap();
    gray_nitf.write_headers(&mut gray_file).unwrap();
    gray_nitf.image_segments[0]
        .write_data(&mut gray_file, &gray.into_raw())
        .unwrap();
}
