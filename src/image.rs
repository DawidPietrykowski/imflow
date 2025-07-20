use exiftool::g2::ExifData;
use image::DynamicImage;
use image::ImageBuffer;
use image::Rgba;
use image::imageops::FilterType;
use image::metadata::Orientation;
use itertools::Itertools;
use jpegxl_rs::Endianness;
use jpegxl_rs::decode::PixelFormat;
use jpegxl_rs::decoder_builder;
use libheif_rs::ItemId;
use libheif_rs::{HeifContext, LibHeif, RgbChroma};
use rexiv2::Metadata;
use rexiv2::is_exif_tag;
use sha2::Digest;
use sha2::Sha256;
use sha2::digest::consts::U32;
use sha2::digest::generic_array::GenericArray;
use zune_image::codecs::jpeg::JpegDecoder;
use zune_image::codecs::qoi::zune_core::colorspace::ColorSpace;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions;

use std::env;
use std::fmt::Display;
// use std::fmt::Write;
use std::fs;
use std::fs::File;
use std::fs::read;
use std::hash::Hash;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

#[derive(Clone, Eq, Hash, PartialEq, PartialOrd)]
pub enum ImageFormat {
    Jpg,
    Jxl,
    Heif,
}

impl Display for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageFormat::Jpg => f.write_str("JPG"),
            ImageFormat::Jxl => f.write_str("JXL"),
            ImageFormat::Heif => f.write_str("HEIF"),
        }
    }
}

#[derive(Clone)]
pub struct ImageData {
    pub path: PathBuf,
    pub format: ImageFormat,
    pub embedded_thumbnail: bool,
    pub orientation: Orientation,
    pub hash: GenericArray<u8, U32>,
    pub rating: i32,
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

    pub fn get_hash_str(&self) -> String {
        format!("{:x}", self.hash)
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
    pub orientation: Orientation,
}

pub fn get_rating(image: &ImageData) -> i32 {
    if let Ok(meta) = Metadata::new_from_path(&image.path) {
        meta.get_tag_numeric("Xmp.xmp.Rating")
    } else {
        0
    }
}

pub fn get_orientation(path: &PathBuf) -> Orientation {
    Metadata::new_from_path(path).map_or(Orientation::NoTransforms, |meta| {
        Orientation::from_exif(meta.get_orientation() as u8).unwrap()
    })
}

pub fn swap_wh<T>(width: T, height: T, orientation: Orientation) -> (T, T) {
    if [
        Orientation::Rotate90,
        Orientation::Rotate270,
        // Orientation::Rotate90FlipH,
        // Orientation::Rotate270FlipH,
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
    if path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with(&['.'])
    {
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
    // sleep(Duration::from_millis(500));
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
            // TODO: convert
            // let orientation = metadata.orientation;
            let orientation = Orientation::NoTransforms;

            let rgba_buffer = vec_u8_to_u32(buffer);

            println!("Total JXL loading time: {:?}", total_start.elapsed());

            ImflowImageBuffer {
                width,
                height,
                rgba_buffer,
                rating,
                orientation,
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

            let orientation = image.orientation;
            let rgba_buffer = vec_u8_to_u32(buffer);
            println!("Total loading time: {:?}", total_start.elapsed());
            println!("Orientation: {:?}", image.orientation);
            ImflowImageBuffer {
                width,
                height,
                rgba_buffer,
                rating,
                orientation,
            }
        }
    }
}

fn vec_u8_to_u32(buffer: Vec<u8>) -> Vec<u32> {
    let rgba_buffer = unsafe {
        Vec::from_raw_parts(
            buffer.as_ptr() as *mut u32,
            buffer.len() / 4,
            buffer.len() / 4,
        )
    };
    std::mem::forget(buffer);
    rgba_buffer
    // bytemuck::cast_vec(buffer)
}

fn vec_u32_to_u8(buffer: Vec<u32>) -> Vec<u8> {
    let rgba_buffer = unsafe {
        Vec::from_raw_parts(
            buffer.as_ptr() as *mut u8,
            buffer.len() * 4,
            buffer.len() * 4,
        )
    };
    std::mem::forget(buffer);
    rgba_buffer
    // bytemuck::cast_vec(buffer)
}

pub fn image_to_rgba_buffer(img: DynamicImage) -> Vec<u32> {
    let flat: ImageBuffer<Rgba<u8>, Vec<u8>> = img.to_rgba8();
    vec_u8_to_u32(flat.into_vec())
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
                let rating = meta.get_tag_numeric("Xmp.xmp.Rating");
                Some(ImageData {
                    path,
                    format,
                    embedded_thumbnail,
                    orientation,
                    hash,
                    rating,
                })
            } else {
                None
            }
        })
        .collect::<Vec<ImageData>>()
}

pub fn check_embedded_thumbnail(path: &PathBuf) -> bool {
    Metadata::new_from_path(path).map_or(false, |meta| meta.get_preview_images().is_some())
}

pub fn get_embedded_thumbnail(image: &ImageData) -> Option<Vec<u8>> {
    let meta = Metadata::new_from_path(&image.path).ok()?;

    let width = meta.get_pixel_width();
    let height = meta.get_pixel_height();
    println!("image: {}", width as f32 / height as f32);

    meta.get_preview_images()?.first().and_then(|preview| {
        let width = preview.get_width();
        let height = preview.get_height();
        println!("thumbnail: {}", width as f32 / height as f32);
        preview.get_data().ok()
    })
}

pub fn load_thumbnail(path: &ImageData) -> ImflowImageBuffer {
    let cache_path = path.get_cache_path();
    let mut buffer: Option<Vec<u8>> = None;
    if cache_path.exists() {
        let read_bytes = fs::read(&cache_path).unwrap();
        if read_bytes.len() != 0 {
            buffer = Some(read_bytes);
        }
    }
    if let Some(bytes) = buffer {
        let width = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let orientation =
            Orientation::from_exif(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as u8)
                .unwrap_or(Orientation::NoTransforms);
        let (ptr, len, cap) = bytes.into_raw_parts();
        assert!(ptr.align_offset(4) == 0);
        let buffer_u32 = unsafe {
            Vec::from_raw_parts(
                (ptr as usize + 12) as *mut u32,
                (len - 12) / 4,
                (cap - 12) / 4,
            )
        };

        assert_eq!(width * height, buffer_u32.len());

        return ImflowImageBuffer {
            width,
            height,
            rgba_buffer: buffer_u32,
            rating: 0,
            orientation,
        };
    }
    let thumbnail = if path.format == ImageFormat::Heif {
        load_heif(path, true)
    } else {
        load_thumbnail_exif(path).unwrap_or_else(|| load_thumbnail_full(path))
    };

    save_thumbnail(&cache_path, thumbnail.clone());
    thumbnail
}

pub fn load_thumbnail_exif(path: &ImageData) -> Option<ImflowImageBuffer> {
    if let Some(thumbnail) = get_embedded_thumbnail(path) {
        let decoder = image::ImageReader::new(Cursor::new(thumbnail))
            .with_guessed_format()
            .unwrap();
        let image = decoder.decode().unwrap();
        let orientation = path.orientation;

        let width = image.width();
        let height = image.height();
        // TODO: extract from image
        let ratio_image = 1.5;
        let ratio_thumbnail = width as f32 / height as f32;
        let crop = ratio_thumbnail / ratio_image;
        let start = ((0.5 - (crop / 2.0)) * height as f32).round();
        let cropped_height = (height as f32 * crop) as u32;
        let mut image = image.crop_imm(0, start as u32, width, cropped_height);

        image.apply_orientation(orientation);
        let width: usize = image.width() as usize;
        let height: usize = image.height() as usize;
        let rgba_buffer = image_to_rgba_buffer(image);
        let rating = get_rating(path.into());

        Some(ImflowImageBuffer {
            width,
            height,
            rgba_buffer,
            rating,
            orientation,
        })
    } else {
        None
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
        .resize_to_fill(720, 720, FilterType::Nearest);
    let width = image.width() as usize;
    let height = image.height() as usize;
    let start = std::time::Instant::now();
    let buffer = image_to_rgba_buffer(image);
    println!("Elapsed: {:?}", start.elapsed());
    let rating = get_rating(path.into());
    let orientation = path.orientation;

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: buffer,
        rating,
        orientation,
    }
}

pub fn load_heif(path: &ImageData, resize: bool) -> ImflowImageBuffer {
    let lib_heif = LibHeif::new();
    let ctx = HeifContext::read_from_file(path.path.to_str().unwrap()).unwrap();
    let mut orientation = Orientation::NoTransforms;

    let image = if resize {
        let binding = ctx.top_level_image_handles();
        let handle = binding.get(0).unwrap();
        let thumbnail_count = handle.number_of_thumbnails() as u32;
        let mut thumbnail_ids = vec![0u32, thumbnail_count];
        handle.thumbnail_ids(&mut thumbnail_ids);
        let handle = &handle.thumbnail(thumbnail_ids[0]).unwrap();

        let width = handle.width();
        let height = handle.height();
        let new_width: u32;
        let new_height: u32;
        const VAR_NAME: f32 = 640 as f32;
        if width > height {
            let scale = VAR_NAME / width as f32;
            new_width = VAR_NAME as u32;
            new_height = (height as f32 * scale) as u32;
        } else {
            let scale = VAR_NAME / height as f32;
            new_height = VAR_NAME as u32;
            new_width = (width as f32 * scale) as u32;
        }

        lib_heif
            .decode(handle, libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba), None)
            .unwrap()
            .scale(new_width, new_height, None)
            .unwrap()
    } else {
        let binding = ctx.top_level_image_handles();
        let handle = binding.get(0).unwrap();

        // Get Exif
        let mut meta_ids: Vec<ItemId> = vec![0; 1];
        let count = handle.metadata_block_ids(&mut meta_ids, b"Exif");
        assert_eq!(count, 1);
        if let Ok(exif) = handle.metadata(meta_ids[0]) {
            if let Ok(metadata) = rexiv2::Metadata::new_from_buffer(&exif) {
                orientation = Orientation::from_exif(metadata.get_orientation() as u8).unwrap();
            }
        }

        lib_heif
            .decode(handle, libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba), None)
            .unwrap()
    };

    assert_eq!(
        image.color_space(),
        Some(libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba)),
    );

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

    let width = interleaved_plane.width as usize;
    let height = interleaved_plane.height as usize;
    let u32_slice = slice_u8_to_u32(rgba_buffer);

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: u32_slice.to_vec(),
        rating,
        orientation,
    }
}

fn slice_u8_to_u32(rgba_buffer: &[u8]) -> &[u32] {
    let u32_slice = unsafe {
        std::slice::from_raw_parts(rgba_buffer.as_ptr() as *const u32, rgba_buffer.len() / 4)
    };
    u32_slice
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
    let mut file = File::create(path).unwrap();
    let u8_buffer = vec_u32_to_u8(image.rgba_buffer);
    file.write(&(image.width as u32).to_le_bytes()).unwrap();
    file.write(&(image.height as u32).to_le_bytes()).unwrap();
    file.write(&(image.orientation.to_exif() as u32).to_le_bytes())
        .unwrap();
    file.write(&u8_buffer).unwrap();
}
