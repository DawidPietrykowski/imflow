use ffmpeg_next as ffmpeg;
use ffmpeg_next::ffi;
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
use log::debug;
use log::warn;
use rexiv2::Metadata;
use sha2::Digest;
use sha2::Sha256;
use sha2::digest::consts::U32;
use sha2::digest::generic_array::GenericArray;
use thiserror::Error;
use zune_image::codecs::jpeg::JpegDecoder;
use zune_image::codecs::qoi::zune_core::colorspace::ColorSpace;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions;

use std::cmp::max;
use std::env;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::fs::read;
use std::hash::Hash;
use std::io;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use crate::utils::round_to_4_multiple;
use crate::utils::slice_u8_to_u32;
use crate::utils::vec_u8_to_u32;
use crate::utils::vec_u32_to_u8;
use crate::xmp::read_rating_xmp;

const EXIF_TAGLIST_TAG: &str = "Xmp.digiKam.TagsList";
const EXIF_RATING_TAG: &str = "Xmp.xmp.Rating";

#[derive(Clone, Eq, Hash, PartialEq, PartialOrd)]
pub enum ImageFormat {
    Jpg,
    Jxl,
    Heif,
    Video,
}

impl Display for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageFormat::Jpg => f.write_str("JPG"),
            ImageFormat::Jxl => f.write_str("JXL"),
            ImageFormat::Heif => f.write_str("HEIF"),
            ImageFormat::Video => f.write_str("VID"),
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
    pub tags: Vec<String>,
}

impl ImageData {
    pub fn get_cache_path(&self) -> PathBuf {
        let home_dir = PathBuf::from_str(&env::var("HOME").unwrap()).unwrap();
        let cache_dir = home_dir.join(".cache/imflow");
        let hash_hex = format!("{:x}", self.hash);
        cache_dir.join(hash_hex).to_path_buf()
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
    pub orientation: Orientation,
}

pub fn get_rating(image: &ImageData) -> i32 {
    if let Ok(meta) = Metadata::new_from_path(&image.path) {
        meta.get_tag_numeric(EXIF_RATING_TAG)
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

fn get_format(path: &Path) -> Option<ImageFormat> {
    if !path.is_file() {
        return None;
    }
    if path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with(['.'])
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
    } else if ["mp4", "mov", "avi"].contains(extension) {
        Some(ImageFormat::Video)
    } else {
        None
    }
}

pub fn load_image(image: &ImageData) -> Result<ImflowImageBuffer, MediaLoadError> {
    match image.format {
        ImageFormat::Heif => Ok(load_heif(image, false)),
        ImageFormat::Jxl => load_jxl(image),
        ImageFormat::Jpg => load_jpg(image),
        ImageFormat::Video => Ok(load_thumbnail_video(&image.path).unwrap()),
    }
}

#[derive(Error, Debug)]
pub enum MediaLoadError {
    #[error("Media file read error")]
    Io(#[from] io::Error),
    #[error("Media file decoding error")]
    Decoding(String),
}

fn load_jpg(image: &ImageData) -> Result<ImflowImageBuffer, MediaLoadError> {
    let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);

    let file = read(&image.path)?;
    let mut decoder = JpegDecoder::new(&file);
    decoder.set_options(options);

    decoder
        .decode_headers()
        .map_err(|e| MediaLoadError::Decoding(format!("Failed to decode JPEG headers: {}", e)))?;
    let info = decoder
        .info()
        .ok_or_else(|| MediaLoadError::Decoding("Failed to read JPEG info".to_string()))?;

    let width = info.width as usize;
    let height = info.height as usize;
    let mut buffer: Vec<u8> = vec![0; width * height * 4];
    decoder
        .decode_into(buffer.as_mut_slice())
        .map_err(|e| MediaLoadError::Decoding(e.to_string()))?;

    let orientation = image.orientation;
    let rgba_buffer = vec_u8_to_u32(buffer);

    Ok(ImflowImageBuffer {
        width,
        height,
        rgba_buffer,
        orientation,
    })
}

fn load_jxl(image: &ImageData) -> Result<ImflowImageBuffer, MediaLoadError> {
    let file = read(&image.path)?;

    let runner = jpegxl_rs::ThreadsRunner::default();
    let decoder = decoder_builder()
        .parallel_runner(&runner)
        .pixel_format(PixelFormat {
            num_channels: 4,
            endianness: Endianness::Big,
            align: 8,
        })
        .build()
        .map_err(|e| MediaLoadError::Decoding(format!("Failed to create JXL decoder: {}", e)))?;

    let (metadata, buffer) = decoder
        .decode_with::<u8>(&file)
        .map_err(|e| MediaLoadError::Decoding(format!("Failed to decode JXL image: {}", e)))?;
    let rgba_buffer = vec_u8_to_u32(buffer);

    let width = metadata.width as usize;
    let height = metadata.height as usize;
    // TODO: convert
    // let orientation = metadata.orientation;
    let orientation = image.orientation;

    Ok(ImflowImageBuffer {
        width,
        height,
        rgba_buffer,
        orientation,
    })
}

pub fn image_to_rgba_buffer(img: DynamicImage) -> Vec<u32> {
    let flat: ImageBuffer<Rgba<u8>, Vec<u8>> = img.to_rgba8();
    vec_u8_to_u32(flat.into_vec())
}

fn load_media_files(dir: &PathBuf) -> Result<Vec<PathBuf>, io::Error> {
    if dir.is_file() {
        return Ok(vec![dir.clone()]);
    }
    let mut all_entries = Vec::<PathBuf>::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            all_entries.append(&mut load_media_files(&entry.path())?);
        } else {
            all_entries.push(entry.path());
        }
    }
    Ok(all_entries)
}

pub fn load_available_images(dir: PathBuf) -> Result<Vec<ImageData>, io::Error> {
    let images: Vec<ImageData> = load_media_files(&dir)?
        .iter()
        .sorted()
        .filter_map(|path| {
            let format = get_format(path)?;
            let Ok(meta) = Metadata::new_from_path(path) else {
                warn!("Image has no metadata, skipping: {:?}", path);
                return None;
            };
            let embedded_thumbnail = if format == ImageFormat::Heif {
                let ctx = HeifContext::read_from_file(path.to_str().unwrap()).unwrap();
                let binding = ctx.top_level_image_handles();
                let handle = binding.first().unwrap();
                handle.number_of_thumbnails() > 0
            } else if format == ImageFormat::Video {
                false
            } else {
                meta.get_preview_images().is_some()
            };
            let mut orientation = Orientation::from_exif(meta.get_orientation() as u8)
                .unwrap_or(Orientation::NoTransforms);
            if format == ImageFormat::Video {
                orientation = load_thumbnail_video(path).unwrap().orientation;
                debug!("video orientation: {:?}, {:?}", orientation, path);
            }
            let hash = get_file_hash(path);
            let tags = meta
                .get_tag_multiple_strings(EXIF_TAGLIST_TAG)
                .unwrap_or_default();
            let rating = match format {
                ImageFormat::Video => read_rating_xmp(path.clone()).unwrap_or(0),
                _ => meta.get_tag_numeric(EXIF_RATING_TAG),
            };
            Some(ImageData {
                path: path.clone(),
                format,
                embedded_thumbnail,
                orientation, // TODO: we might not know the final rotation at this point (such as with videos)
                hash,
                rating,
                tags,
            })
        })
        .collect();
    Ok(images)
}

pub fn check_embedded_thumbnail(path: &PathBuf) -> bool {
    Metadata::new_from_path(path).is_ok_and(|meta| meta.get_preview_images().is_some())
}

pub fn get_embedded_thumbnail(image: &ImageData) -> Option<Vec<u8>> {
    let meta = Metadata::new_from_path(&image.path).ok()?;
    meta.get_preview_images()?
        .first()
        .and_then(|preview| preview.get_data().ok())
}

pub fn load_thumbnail(path: &ImageData) -> ImflowImageBuffer {
    let cache_path = path.get_cache_path();
    let mut buffer: Option<Vec<u8>> = None;
    if cache_path.exists() {
        let read_bytes = fs::read(&cache_path).unwrap();
        if read_bytes.is_empty() {
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
            orientation,
        };
    }
    let thumbnail = match path.format {
        ImageFormat::Heif => load_heif(path, true),
        ImageFormat::Video => load_thumbnail_video(&path.path).unwrap(),
        _ => load_thumbnail_exif(path).unwrap_or_else(|| load_thumbnail_full(path)),
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

        Some(ImflowImageBuffer {
            width,
            height,
            rgba_buffer,
            orientation: Orientation::NoTransforms,
        })
    } else {
        None
    }
}

pub fn load_thumbnail_video(path: &PathBuf) -> Option<ImflowImageBuffer> {
    let mut ictx = ffmpeg::format::input(&path).unwrap();
    let best_video_stream_index = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .map(|stream| stream.index())
        .unwrap();
    let stream = ictx.stream(best_video_stream_index).unwrap();
    let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .unwrap()
        .decoder()
        .video()
        .unwrap();

    let mut decoded_frame = None;
    let mut orientation = None;
    for (stream, packet) in ictx.packets() {
        if stream.index() == best_video_stream_index {
            if let Some(side_data) = stream
                .side_data()
                .find(|s| s.kind() == ffmpeg_next::packet::side_data::Type::DisplayMatrix)
            {
                let mat_ptr = side_data.data().as_ptr() as *const ffi::__int32_t;
                let angle = unsafe { ffi::av_display_rotation_get(mat_ptr) } as i64;
                let angle = ((angle % 360) + 360) % 360;
                debug!("video frame angle: {}, {:?}", angle, path);
                orientation = Some(match angle {
                    90 => Orientation::Rotate90,
                    180 => Orientation::Rotate180,
                    270 => Orientation::Rotate90,
                    _ => panic!(),
                })
            };
            if decoder.send_packet(&packet).is_ok() {
                let mut decoded = ffmpeg::frame::Video::empty();
                if decoder.receive_frame(&mut decoded).is_ok() {
                    decoded_frame = Some(decoded);
                    break;
                }
            }
        }
    }

    debug!("orientation: {:?}, {:?}\n", orientation, path);

    let key_frame = decoded_frame.unwrap();

    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::RGBA,
        round_to_4_multiple(decoder.width() / 4),
        round_to_4_multiple(decoder.height() / 4),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )
    .ok()?;

    let mut rgba_frame = ffmpeg::frame::Video::empty();
    scaler.run(&key_frame, &mut rgba_frame).ok()?;

    let buffer = rgba_frame.plane::<[u8; 4]>(0).as_flattened();
    let buffer = vec_u8_to_u32(buffer.to_vec());
    Some(ImflowImageBuffer {
        width: rgba_frame.width() as usize,
        height: rgba_frame.height() as usize,
        rgba_buffer: buffer.to_vec(),
        orientation: orientation.unwrap_or(Orientation::NoTransforms),
    })
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
    debug!("Elapsed: {:?}", start.elapsed());
    let orientation = path.orientation;

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: buffer,
        orientation,
    }
}

pub fn load_heif(path: &ImageData, resize: bool) -> ImflowImageBuffer {
    let lib_heif = LibHeif::new();
    let ctx = HeifContext::read_from_file(path.path.to_str().unwrap()).unwrap();
    let mut orientation = Orientation::NoTransforms;
    let binding = ctx.top_level_image_handles();
    let handle = binding.first().unwrap();
    let thumbnail_count = handle.number_of_thumbnails() as u32;

    let mut image = if resize && thumbnail_count > 0 {
        let mut thumbnail_ids = vec![0u32, thumbnail_count];
        let cnt = handle.thumbnail_ids(&mut thumbnail_ids);
        assert_ne!(cnt, 0);
        let handle = &handle.thumbnail(thumbnail_ids[0]).unwrap();

        let width = handle.width();
        let height = handle.height();
        let new_width: u32;
        let new_height: u32;
        const VAR_NAME: f32 = 640f32;
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
        let handle = binding.first().unwrap();

        // Get Exif
        let mut meta_ids: Vec<ItemId> = vec![0; 1];
        let count = handle.metadata_block_ids(&mut meta_ids, b"Exif");
        assert_eq!(count, 1);
        if let Ok(exif) = handle.metadata(meta_ids[0])
            && let Ok(metadata) = rexiv2::Metadata::new_from_buffer(&exif)
        {
            orientation = Orientation::from_exif(metadata.get_orientation() as u8).unwrap();
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
    if resize && thumbnail_count == 0 {
        const MAX: usize = 600;
        let mut width = image.width() as usize;
        let mut height = image.height() as usize;
        let scale = max(width, height) as f32 / MAX as f32;
        width = round_to_4_multiple((width as f32 / scale) as usize);
        height = round_to_4_multiple((height as f32 / scale) as usize);
        image = image.scale(width as u32, height as u32, None).unwrap();
    }

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
        orientation,
    }
}

pub fn get_file_hash(path: &PathBuf) -> GenericArray<u8, U32> {
    let mut file = File::open(path).unwrap();
    let mut buf = [0u8; 16 * 1024];
    let read_bytes = file.read(&mut buf).unwrap();
    assert_ne!(read_bytes, 0);

    let mut hasher = Sha256::new();
    hasher.update(buf);
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
    file.write_all(&(image.width as u32).to_le_bytes()).unwrap();
    file.write_all(&(image.height as u32).to_le_bytes()).unwrap();
    file.write_all(&(image.orientation.to_exif() as u32).to_le_bytes())
        .unwrap();
    file.write_all(&u8_buffer).unwrap();
}
