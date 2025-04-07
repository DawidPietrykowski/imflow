use image::DynamicImage;
use image::ImageDecoder;
use image::RgbaImage;
use image::imageops::FilterType;
use image::metadata::Orientation;
use jpegxl_rs::Endianness;
use jpegxl_rs::decode::Data;
use jpegxl_rs::decode::PixelFormat;
use jpegxl_rs::decode::Pixels;
use jpegxl_rs::decoder_builder;
use libheif_rs::{HeifContext, LibHeif, RgbChroma};
use rexiv2::Metadata;
use zune_image::codecs::jpeg::JpegDecoder;
use zune_image::codecs::jpeg_xl::JxlDecoder;
use zune_image::codecs::qoi::zune_core::colorspace::ColorSpace;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions;
use zune_image::traits::DecoderTrait;

use std::fs;
use std::fs::File;
use std::fs::read;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::mem;
use std::path::PathBuf;
use std::time::Instant;

pub struct ImflowImageBuffer {
    pub width: usize,
    pub height: usize,
    pub rgba_buffer: Vec<u32>,
    pub rating: i32,
}

// pub fn create_iced_handle(width: u32, height: u32, rgba: Vec<u8>) -> Handle {
//     Handle::from_rgba(width, height, rgba)
// }

pub fn get_rating(filename: &PathBuf) -> i32 {
    // // Use xmp-toolkit for video files
    // if is_video(&filename) {
    //     return Ok(read_rating_xmp(filename.clone()).unwrap_or(0));
    // }

    // Use rexiv2 for image files
    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) => {
            let rating = meta.get_tag_numeric("Xmp.xmp.Rating");
            rating
        }
        Err(e) => panic!("{:?}", e),
    }
}

pub fn get_orientation(filename: &PathBuf) -> u8 {
    // // Use xmp-toolkit for video files
    // if is_video(&filename) {
    //     return Ok(read_rating_xmp(filename.clone()).unwrap_or(0));
    // }

    // Use rexiv2 for image files
    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) => meta.get_orientation() as u8,
        Err(e) => panic!("{:?}", e),
    }
}

fn swap_wh<T>(width: T, height: T, orientation: Orientation) -> (T, T) {
    if [
        Orientation::Rotate90,
        Orientation::Rotate270,
        Orientation::Rotate90FlipH,
        Orientation::Rotate270FlipH,
    ]
    .contains(&orientation)
    {
        return (height, width);
    }
    (width, height)
}

pub fn load_image(path: &PathBuf) -> ImflowImageBuffer {
    let total_start = Instant::now();

    if is_heif(path) {
        let img = load_heif(path, false);
        let total_time = total_start.elapsed();
        println!("Total HEIF loading time: {:?}", total_time);
        return img;
    }

    let width: usize;
    let height: usize;
    let rating = get_rating(path);
    if is_jxl(path) {
        let file = read(path).unwrap();
        use jpegxl_rs::ThreadsRunner;
        let runner = ThreadsRunner::default();
        let decoder = decoder_builder()
            .parallel_runner(&runner)
            .pixel_format(PixelFormat {
                num_channels: 4,
                endianness: Endianness::Big,
                align: 8,
            })
            .build()
            .unwrap();

        let (metadata, buffer) = decoder.decode_with::<u8>(&file).unwrap();
        width = metadata.width as usize;
        height = metadata.height as usize;

        let rgba_buffer = unsafe {
            Vec::from_raw_parts(
                buffer.as_ptr() as *mut u32,
                buffer.len() / 4,
                buffer.len() / 4,
            )
        };
        std::mem::forget(buffer);

        println!("Total loading time: {:?}", total_start.elapsed());

        let rating = get_rating(path);

        ImflowImageBuffer {
            width,
            height,
            rgba_buffer,
            rating,
        }
    } else {
        let mut buffer: Vec<u8>;
        let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let file = read(path.clone()).unwrap();
        let mut decoder = JpegDecoder::new(&file);
        decoder.set_options(options);

        decoder.decode_headers().unwrap();
        let info = decoder.info().unwrap();
        width = info.width as usize;
        height = info.height as usize;
        buffer = vec![0; width * height * 4];
        decoder.decode_into(buffer.as_mut_slice()).unwrap();

        let orientation_start = Instant::now();
        // TODO: Optimize rotation
        let orientation =
            Orientation::from_exif(get_orientation(path)).unwrap_or(Orientation::NoTransforms);
        let image = RgbaImage::from_raw(width as u32, height as u32, buffer).unwrap();
        let mut dynamic_image = DynamicImage::from(image);
        dynamic_image.apply_orientation(orientation);
        let buffer = dynamic_image.as_rgba8().unwrap();
        let (width, height) = swap_wh(width, height, orientation);
        let orientation_time = orientation_start.elapsed();

        // Reinterpret to avoid copying
        let rgba_buffer = unsafe {
            Vec::from_raw_parts(
                buffer.as_ptr() as *mut u32,
                buffer.len() / 4,
                buffer.len() / 4,
            )
        };
        std::mem::forget(dynamic_image);
        let total_time = total_start.elapsed();
        println!("Orientation time: {:?}", orientation_time);
        println!("Total loading time: {:?}", total_time);
        ImflowImageBuffer {
            width,
            height,
            rgba_buffer,
            rating,
        }
    }
}

pub fn image_to_rgba_buffer(img: DynamicImage) -> Vec<u32> {
    let flat = img.to_rgba8();
    let mut buffer = flat.to_vec();
    let vec = unsafe {
        Vec::from_raw_parts(
            buffer.as_mut_ptr() as *mut u32,
            buffer.len() / 4,
            buffer.len() / 4,
        )
    };
    mem::forget(buffer);
    vec
}

pub fn load_available_images(dir: PathBuf) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|f| f.unwrap().path())
        .filter(is_image)
        .collect();
    files.sort();
    files
}

pub fn get_embedded_thumbnail(path: PathBuf) -> Option<Vec<u8>> {
    let meta = rexiv2::Metadata::new_from_path(path);
    match meta {
        Ok(meta) => {
            if let Some(previews) = meta.get_preview_images() {
                for preview in previews {
                    return Some(preview.get_data().unwrap());
                }
            }
            None
        }
        Err(_) => None,
    }
}

fn is_image(path: &PathBuf) -> bool {
    if !path.is_file() {
        return false;
    }
    ["jpg", "jxl", "heic", "heif"].contains(
        &path
            .extension()
            .unwrap()
            .to_ascii_lowercase()
            .to_str()
            .unwrap(),
    )
}

fn is_heif(path: &PathBuf) -> bool {
    ["heif", "heic"].contains(
        &path
            .extension()
            .unwrap()
            .to_ascii_lowercase()
            .to_str()
            .unwrap(),
    )
}

fn is_jxl(path: &PathBuf) -> bool {
    ["jxl", "jpgxl"].contains(
        &path
            .extension()
            .unwrap()
            .to_ascii_lowercase()
            .to_str()
            .unwrap(),
    )
}

pub fn load_thumbnail(path: &PathBuf) -> ImflowImageBuffer {
    if is_heif(path) {
        return load_heif(path, true);
    }
    match load_thumbnail_exif(path) {
        Some(thumbnail) => return thumbnail,
        None => load_thumbnail_full(path),
    }
}

pub fn load_thumbnail_exif(path: &PathBuf) -> Option<ImflowImageBuffer> {
    match get_embedded_thumbnail(path.clone()) {
        Some(thumbnail) => {
            let decoder = image::ImageReader::new(Cursor::new(thumbnail))
                .with_guessed_format()
                .unwrap();
            let image = decoder.decode().unwrap();

            let width: usize = image.width() as usize;
            let height: usize = image.height() as usize;
            let flat = image.into_rgba8().into_raw();
            let mut buffer = flat.to_vec();
            let buffer_u32 = unsafe {
                Vec::from_raw_parts(
                    buffer.as_mut_ptr() as *mut u32,
                    buffer.len() / 4,
                    buffer.len() / 4,
                )
            };

            let rating = get_rating(path.into());

            Some(ImflowImageBuffer {
                width,
                height,
                rgba_buffer: buffer_u32,
                rating,
            })
        }
        _ => None,
    }
}

pub fn load_thumbnail_full(path: &PathBuf) -> ImflowImageBuffer {
    let file = BufReader::new(File::open(path).unwrap());
    let reader = image::ImageReader::new(file);
    let image = reader
        .with_guessed_format()
        .unwrap()
        .decode()
        .unwrap()
        .resize(640, 480, FilterType::Nearest);
    let width = image.width() as usize;
    let height = image.height() as usize;
    let buffer = image_to_rgba_buffer(image);
    let rating = get_rating(path.into());

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: buffer,
        rating,
    }
}

pub fn load_heif(path: &PathBuf, resize: bool) -> ImflowImageBuffer {
    let lib_heif = LibHeif::new();
    let ctx = HeifContext::read_from_file(path.to_str().unwrap()).unwrap();
    let handle = ctx.primary_image_handle().unwrap();
    // assert_eq!(handle.width(), 1652);
    // assert_eq!(handle.height(), 1791);

    // Get Exif
    // let mut meta_ids: Vec<ItemId> = vec![0; 1];
    // let count = handle.metadata_block_ids(&mut meta_ids, b"Exif");
    // assert_eq!(count, 1);
    // let exif: Vec<u8> = handle.metadata(meta_ids[0]).unwrap();

    // Decode the image
    let mut image = lib_heif
        .decode(&handle, libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba), None)
        .unwrap();
    assert_eq!(
        image.color_space(),
        Some(libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba)),
    );

    // Scale the image
    if resize {
        image = image.scale(640, 480, None).unwrap();
        assert_eq!(image.width(), 640);
        assert_eq!(image.height(), 480);
    }

    let width = image.width() as usize;
    let height = image.height() as usize;
    let rating = get_rating(path);

    // Get "pixels"
    let planes = image.planes();
    let interleaved_plane = planes.interleaved.unwrap();
    assert!(!interleaved_plane.data.is_empty());
    assert!(interleaved_plane.stride > 0);

    let rgba_buffer = interleaved_plane.data;
    // Create a slice of u32 from the u8 slice
    let u32_slice = unsafe {
        std::slice::from_raw_parts(rgba_buffer.as_ptr() as *const u32, rgba_buffer.len() / 4)
    };

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: u32_slice.to_vec(),
        rating,
    }
}

// fn load_jxl(path: &PathBuf) -> ImflowImageBuffer {
//     let file = BufReader::new(File::open(path).unwrap());
//     let decoder = JxlDecoder::try_new(file, DecoderOptions::new_fast()).unwrap();
//     // let reader = image::ImageReader::new(file);
//     let image = decoder
//         .decode()
//         .unwrap();
//     let width = image.width() as usize;
//     let height = image.height() as usize;
//     let buffer = image_to_rgba_buffer(image);
//     let rating = get_rating(path.into());

//     ImflowImageBuffer {
//         width,
//         height,
//         rgba_buffer: buffer,
//         rating,
//     }
// }
