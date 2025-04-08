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
use sha2::Digest;
use sha2::Sha256;
use sha2::digest::consts::U32;
use sha2::digest::generic_array::GenericArray;
use zune_image::codecs::jpeg::JpegDecoder;
use zune_image::codecs::qoi::zune_core::colorspace::ColorSpace;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions;

use std::env;
use std::fs;
use std::fs::File;
use std::fs::read;
use std::hash::Hash;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::mem;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

#[derive(Clone, Eq, Hash, PartialEq, PartialOrd)]
pub enum ImageFormat {
    Jpg,
    Jxl,
    Heif,
}

#[derive(Clone)]
pub struct ImageData {
    pub path: PathBuf,
    pub format: ImageFormat,
    pub embedded_thumbnail: bool,
    pub orientation: Orientation,
    pub hash: GenericArray<u8, U32>,
}

impl ImageData {
    pub fn get_cache_path(&self) -> PathBuf {
        let home_dir = PathBuf::from_str(&env::var("HOME").unwrap()).unwrap();
        let cache_dir = home_dir.join(".cache/imflow");
        if !cache_dir.exists() {
            fs::create_dir(&cache_dir).unwrap();
        }
        let hash_hex = format!("{:x}", self.hash);
        return cache_dir.join(hash_hex).to_path_buf();
    }
}

impl Hash for ImageData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(self.path.to_str().unwrap().as_bytes());
        state.write(self.hash.as_slice());
    }
}

impl PartialEq for ImageData {
    fn eq(&self, other: &Self) -> bool {
        self.hash.eq(&other.hash)
    }
}

impl Eq for ImageData {}

#[derive(Clone)]
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
                let embedded_thumbnail = if format == ImageFormat::Heif {
                    let ctx = HeifContext::read_from_file(path.to_str().unwrap()).unwrap();
                    let binding = ctx.top_level_image_handles();
                    let handle = binding.get(0).unwrap();
                    handle.number_of_thumbnails() > 0
                } else {
                    meta.get_preview_images().is_some()
                };
                let orientation = Orientation::from_exif(meta.get_orientation() as u8)
                    .unwrap_or(Orientation::NoTransforms);
                let hash = get_file_hash(&path);
                Some(ImageData {
                    path,
                    format,
                    embedded_thumbnail,
                    orientation,
                    hash,
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
    let cache_path = path.get_cache_path();
    if cache_path.exists() {
        let bytes = fs::read(cache_path).unwrap();
        let width = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let buffer: &[u8] = &bytes[8..];
        let mut buffer_u8 = buffer.to_vec();
        let buffer_u32 = unsafe {
            Vec::from_raw_parts(
                buffer_u8.as_mut_ptr() as *mut u32,
                buffer_u8.len() / 4,
                buffer_u8.len() / 4,
            )
        };
        std::mem::forget(buffer_u8);

        assert_eq!(width * height, buffer_u32.len());

        return ImflowImageBuffer {
            width,
            height,
            rgba_buffer: buffer_u32,
            rating: 0,
        };
    }
    let thumbnail = if path.format == ImageFormat::Heif {
        load_heif(path, true)
    } else {
        load_thumbnail_exif(path).unwrap_or(load_thumbnail_full(path))
    };

    save_thumbnail(&cache_path, thumbnail.clone());
    thumbnail
}

pub fn load_thumbnail_exif(path: &ImageData) -> Option<ImflowImageBuffer> {
    match get_embedded_thumbnail(path) {
        Some(thumbnail) => {
            let decoder = image::ImageReader::new(Cursor::new(thumbnail))
                .with_guessed_format()
                .unwrap();
            let mut image = decoder.decode().unwrap();

            image.apply_orientation(path.orientation);
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
            std::mem::forget(buffer);

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
        .resize_to_fill(1920, 1920, FilterType::Lanczos3);
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

    let image = if resize {
        let binding = ctx.top_level_image_handles();
        let handle = binding.get(0).unwrap();
        let thumbnail_count = handle.number_of_thumbnails() as u32;
        let mut thumbnail_ids = vec![0u32, thumbnail_count];
        handle.thumbnail_ids(&mut thumbnail_ids);
        let handle = &handle.thumbnail(thumbnail_ids[0]).unwrap();

        lib_heif
            .decode(handle, libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba), None)
            .unwrap()
    } else {
        let binding = ctx.top_level_image_handles();
        let handle = binding.get(0).unwrap();

        lib_heif
            .decode(handle, libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba), None)
            .unwrap()
    };

    assert_eq!(
        image.color_space(),
        Some(libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba)),
    );

    let width = image.width() as usize;
    let height = image.height() as usize;

    // Scale the image
    // if resize {
    //     const MAX: usize = 3000;
    //     let scale = max(width, height) as f32 / MAX as f32;
    //     width = (width as f32 / scale) as usize;
    //     height = (height as f32 / scale) as usize;
    //     // image = image.scale(width as u32, height as u32, None).unwrap();
    //     image = image.scale(599, 300, None).unwrap();
    //     width = image.width() as usize;
    //     height = image.height() as usize;
    // }

    let rating = get_rating(path);

    // Get "pixels"
    let planes = image.planes();
    let interleaved_plane = planes.interleaved.unwrap();
    assert!(!interleaved_plane.data.is_empty());
    assert!(interleaved_plane.stride > 0);
    assert_eq!(interleaved_plane.storage_bits_per_pixel, 32);

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

pub fn get_file_hash(path: &PathBuf) -> GenericArray<u8, U32> {
    let mut file = File::open(path).unwrap();
    let mut buf = [0u8; 16 * 1024];
    file.read(&mut buf).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&buf);
    hasher.update(file.metadata().unwrap().len().to_le_bytes());
    hasher.finalize()
}

// TODO: optimize
pub fn save_thumbnail(path: &PathBuf, image: ImflowImageBuffer) {
    let cache_dir = path.parent().unwrap();
    if !cache_dir.exists() {
        fs::create_dir(cache_dir).unwrap();
    }
    println!("path: {:?}", path);
    let mut file = File::create(path).unwrap();
    let buffer = image.rgba_buffer;
    let u8_buffer = unsafe {
        Vec::from_raw_parts(
            buffer.as_ptr() as *mut u8,
            buffer.len() * 4,
            buffer.len() * 4,
        )
    };
    std::mem::forget(buffer);
    file.write(&(image.width as u32).to_le_bytes()).unwrap();
    file.write(&(image.height as u32).to_le_bytes()).unwrap();
    file.write(&u8_buffer).unwrap();
}
