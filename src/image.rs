use iced::widget::image::Handle;
use image::DynamicImage;
use image::imageops::FilterType;
use libheif_rs::{HeifContext, LibHeif, RgbChroma};
use rexiv2::Metadata;
use zune_image::codecs::jpeg::JpegDecoder;
use zune_image::codecs::qoi::zune_core::colorspace::ColorSpace;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions;

use std::fs;
use std::fs::File;
use std::fs::read;
use std::io::BufReader;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Instant;

pub struct ImflowImageBuffer {
    pub width: usize,
    pub height: usize,
    pub rgba_buffer: Vec<u32>,
    pub rating: i32,
}

pub fn create_iced_handle(width: u32, height: u32, rgba: Vec<u8>) -> Handle {
    Handle::from_rgba(width, height, rgba)
}

fn get_rating(filename: &PathBuf) -> i32 {
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

pub fn load_image(path: &PathBuf) -> ImflowImageBuffer {
    let total_start = Instant::now();

    if is_heif(path) {
        let img = load_heif(path, false);
        let total_time = total_start.elapsed();
        println!("Total HEIF loading time: {:?}", total_time);
        return img;
    }

    let file = read(path.clone()).unwrap();
    let mut decoder = JpegDecoder::new(&file);
    let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);
    decoder.set_options(options);

    decoder.decode_headers().unwrap();
    let info = decoder.info().unwrap();
    let width = info.width as usize;
    let height = info.height as usize;

    let mut buffer: Vec<u8> = vec![0; width * height * 4];
    decoder.decode_into(buffer.as_mut_slice()).unwrap();

    // Reinterpret to avoid copying
    let buffer_u32 = unsafe {
        Vec::from_raw_parts(
            buffer.as_mut_ptr() as *mut u32,
            buffer.len() / 4,
            buffer.capacity() / 4,
        )
    };
    std::mem::forget(buffer);

    let total_time = total_start.elapsed();
    println!("Total loading time: {:?}", total_time);

    let rating = get_rating(path);

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: buffer_u32,
        rating,
    }
}

pub fn image_to_rgba_buffer(img: DynamicImage) -> Vec<u32> {
    let flat = img.to_rgba8();
    let mut buffer = flat.to_vec();
    unsafe {
        Vec::from_raw_parts(
            buffer.as_mut_ptr() as *mut u32,
            buffer.len() / 4,
            buffer.len() / 4,
        )
    }
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
    ["jpg", "heic", "heif"].contains(
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
