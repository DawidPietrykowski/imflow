use image::DynamicImage;
use image::RgbaImage;
use image::imageops::FilterType;
use image::metadata::Orientation;
use itertools::Itertools;
use jpegxl_rs::Endianness;
use jpegxl_rs::decode::PixelFormat;
use jpegxl_rs::decoder_builder;
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
use std::mem;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Eq, Hash, PartialEq, PartialOrd)]
pub enum ImageFormat {
    Jpg,
    Jxl,
    Heif,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct ImageData {
    pub path: PathBuf,
    pub format: ImageFormat,
    pub embedded_thumbnail: bool,
    pub orientation: Orientation,
}

pub struct ImflowImageBuffer {
    pub width: usize,
    pub height: usize,
    pub rgba_buffer: Vec<u32>,
    pub rating: i32,
}

pub fn get_rating(image: &ImageData) -> i32 {
    let meta = Metadata::new_from_path(&image.path);
    match meta {
        Ok(meta) => {
            let rating = meta.get_tag_numeric("Xmp.xmp.Rating");
            rating
        }
        Err(e) => panic!("{:?}", e),
    }
}

pub fn get_orientation(path: &PathBuf) -> Orientation {
    let meta = Metadata::new_from_path(path);
    match meta {
        Ok(meta) => Orientation::from_exif(meta.get_orientation() as u8)
            .unwrap_or(Orientation::NoTransforms),
        Err(_) => Orientation::NoTransforms,
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

fn get_format(path: &PathBuf) -> Option<ImageFormat> {
    if !path.is_file() {
        return None;
    }
    let os_str = path.extension().unwrap().to_ascii_lowercase();
    let extension = &os_str.to_str().unwrap();
    if ["heic", "heif"].contains(extension) {
        Some(ImageFormat::Heif)
    } else if ["jpg", "jpeg"].contains(extension) {
        Some(ImageFormat::Jpg)
    } else if ["jxl"].contains(extension) {
        Some(ImageFormat::Jxl)
    } else {
        None
    }
}

pub fn load_image(image: &ImageData) -> ImflowImageBuffer {
    let total_start = Instant::now();

    match image.format {
        ImageFormat::Heif => {
            let img = load_heif(image, false);
            let total_time = total_start.elapsed();
            println!("Total HEIF loading time: {:?}", total_time);
            img
        }
        ImageFormat::Jxl => {
            let rating = get_rating(image);

            let file = read(image.path.clone()).unwrap();
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
            let width = metadata.width as usize;
            let height = metadata.height as usize;

            let rgba_buffer = unsafe {
                Vec::from_raw_parts(
                    buffer.as_ptr() as *mut u32,
                    buffer.len() / 4,
                    buffer.len() / 4,
                )
            };
            std::mem::forget(buffer);

            println!("Total JXL loading time: {:?}", total_start.elapsed());

            ImflowImageBuffer {
                width,
                height,
                rgba_buffer,
                rating,
            }
        }
        ImageFormat::Jpg => {
            let rating = get_rating(image);

            let mut buffer: Vec<u8>;
            let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);
            let file = read(image.path.clone()).unwrap();
            let mut decoder = JpegDecoder::new(&file);
            decoder.set_options(options);

            decoder.decode_headers().unwrap();
            let info = decoder.info().unwrap();
            let width = info.width as usize;
            let height = info.height as usize;
            buffer = vec![0; width * height * 4];
            decoder.decode_into(buffer.as_mut_slice()).unwrap();

            let orientation_start = Instant::now();
            // TODO: Optimize rotation
            let orientation = image.orientation;
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

pub fn load_available_images(dir: PathBuf) -> Vec<ImageData> {
    fs::read_dir(dir)
        .unwrap()
        .map(|f| f.unwrap().path().to_path_buf())
        .sorted()
        .filter_map(|path| {
            if let Some(format) = get_format(&path) {
                let meta = Metadata::new_from_path(&path)
                    .expect(&format!("Image has no metadata: {:?}", path).to_string());
                let embedded_thumbnail = meta.get_preview_images().is_some();
                let orientation = Orientation::from_exif(meta.get_orientation() as u8)
                    .unwrap_or(Orientation::NoTransforms);
                Some(ImageData {
                    path,
                    format,
                    embedded_thumbnail,
                    orientation,
                })
            } else {
                None
            }
        })
        .collect::<Vec<ImageData>>()
}

pub fn check_embedded_thumbnail(path: &PathBuf) -> bool {
    if let Ok(meta) = Metadata::new_from_path(&path) {
        meta.get_preview_images().is_some()
    } else {
        false
    }
}

pub fn get_embedded_thumbnail(image: &ImageData) -> Option<Vec<u8>> {
    let meta = Metadata::new_from_path(&image.path);
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

pub fn load_thumbnail(path: &ImageData) -> ImflowImageBuffer {
    if path.format == ImageFormat::Heif {
        return load_heif(path, true);
    }
    match load_thumbnail_exif(path) {
        Some(thumbnail) => return thumbnail,
        None => load_thumbnail_full(path),
    }
}

pub fn load_thumbnail_exif(path: &ImageData) -> Option<ImflowImageBuffer> {
    match get_embedded_thumbnail(path) {
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

pub fn load_thumbnail_full(path: &ImageData) -> ImflowImageBuffer {
    let file = BufReader::new(File::open(path.path.clone()).unwrap());
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

pub fn load_heif(path: &ImageData, resize: bool) -> ImflowImageBuffer {
    let lib_heif = LibHeif::new();
    let ctx = HeifContext::read_from_file(path.path.to_str().unwrap()).unwrap();
    let handle = ctx.primary_image_handle().unwrap();
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
