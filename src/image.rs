use ffmpeg_next as ffmpeg;
use ffmpeg_next::ffi;
use image::DynamicImage;
use image::ImageBuffer;
use image::ImageDecoder;
use image::ImageReader;
use image::Rgba;
use image::imageops::FilterType;
use image::metadata::Orientation;
// use jxl_oxide::integration::JxlDecoder;
// use jpegxl_rs::Endianness;
// use jpegxl_rs::decode::PixelFormat;
// use jpegxl_rs::decoder_builder;
// use jxl_oxide::integration::JxlDecoder;
use libheif_rs::ItemId;
use libheif_rs::{HeifContext, LibHeif, RgbChroma};
use log::debug;
// use log::warn;
// use rexiv2::Metadata;
// use rexiv2::Metadata;
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
use std::fmt::Debug;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::fs::read;
use std::hash::Hash;
use std::io;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use crate::utils::round_to_4_multiple;
use crate::utils::slice_u8_to_u32;
use crate::utils::vec_u8_to_u32;
use crate::utils::vec_u32_to_u8;
use crate::xmp::read_rating_from_raw_xmp;
use crate::xmp::read_rating_xmp;

// const EXIF_TAGLIST_TAG: &str = "Xmp.digiKam.TagsList";
// const EXIF_RATING_TAG: &str = "Xmp.xmp.Rating";

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

impl Debug for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{}", self).as_str())
    }
}

#[derive(Clone)]
pub struct ImageData {
    pub path: PathBuf,
    pub format: ImageFormat,
    pub embedded_thumbnail: bool,
    // pub orientation: Orientation,
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

impl Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageData")
            .field("path", &self.path)
            .field("format", &self.format)
            .field("embedded_thumbnail", &self.embedded_thumbnail)
            .field("hash", &self.hash)
            .field("rating", &self.rating)
            .field("tags", &self.tags)
            .finish()
    }
}

#[derive(Clone)]
pub struct ImflowImageBuffer {
    pub width: usize,
    pub height: usize,
    pub rgba_buffer: Vec<u32>,
    pub orientation: Orientation,
}

pub fn get_rating(image: &ImageData) -> i32 {
    read_rating_xmp(&image.path).unwrap_or(0)
    // if let Ok(meta) = Metadata::new_from_path(&image.path) {
    //     meta.get_tag_numeric(EXIF_RATING_TAG)
    // } else {
    //     0
    // }
}

// pub fn get_orientation(path: &PathBuf) -> Orientation {
//     rexiv2::Metadata::new_from_path(path).map_or(Orientation::NoTransforms, |meta| {
//         Orientation::from_exif(meta.get_orientation() as u8).unwrap()
//     })
// }

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
    // } else if ["jxl"].contains(extension) {
    //     Some(ImageFormat::Jxl)
    } else if ["mp4", "mov", "avi"].contains(extension) {
        Some(ImageFormat::Video)
    } else {
        None
    }
}

pub fn load_image(image: &Path) -> Result<ImflowImageBuffer, MediaLoadError> {
    let format = get_format(image).unwrap();
    match format {
        ImageFormat::Heif => Ok(load_heif(image, false)),
        ImageFormat::Jxl => load_jxl(image),
        ImageFormat::Jpg => Ok(load_jpg_full(&image)),
        ImageFormat::Video => Ok(load_thumbnail_video(&image).unwrap()),
    }
}

#[derive(Error, Debug)]
pub enum MediaLoadError {
    #[error("Media file read error")]
    Io(#[from] io::Error),
    #[error("Media file decoding error")]
    Decoding(String),
}

use exif::Reader;
use exif::Tag;
fn load_jxl(_image: &Path) -> Result<ImflowImageBuffer, MediaLoadError> {
    // let file = read(&image.path)?;

    // let file = std::fs::File::open(image.path).expect("cannot open file");
    // let mut decoder = JxlDecoder::new(file).expect("cannot decode image");

    // let exif = decoder
    //     .exif_metadata()
    //     .expect("cannot decode Exif metadata");
    // let Some(exif) = exif else {
    //     return Err(MediaLoadError::Decoding("No exif metadata found".to_string()));
    // };

    // let (width, height) = decoder.dimensions();
    // decoder.read_image().unwrap();
    todo!();

    // let exif_reader = Reader::new();
    // let Ok(metadata) = exif_reader.read_raw(exif) else {
    //     return Err(MediaLoadError::Decoding("Exif decoding failed".to_string()));
    // };
    // metadata.get_field(Tag::ReferenceBlackWhite, ifd_num)

    // let runner = jpegxl_rs::ThreadsRunner::default();
    // let decoder = decoder_builder()
    //     .parallel_runner(&runner)
    //     .pixel_format(PixelFormat {
    //         num_channels: 4,
    //         endianness: Endianness::Big,
    //         align: 8,
    //     })
    //     .build()
    //     .map_err(|e| MediaLoadError::Decoding(format!("Failed to create JXL decoder: {}", e)))?;

    // let (metadata, buffer) = decoder
    //     .decode_with::<u8>(&file)
    //     .map_err(|e| MediaLoadError::Decoding(format!("Failed to decode JXL image: {}", e)))?;
    // let rgba_buffer = vec_u8_to_u32(buffer);

    // let width = metadata.width as usize;
    // let height = metadata.height as usize;
    // // TODO: convert
    // // let orientation = metadata.orientation;
    // let orientation = image.orientation;

    // Ok(ImflowImageBuffer {
    //     width,
    //     height,
    //     rgba_buffer,
    //     orientation,
    // })
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
            if get_format(&entry.path()).is_some() {
                all_entries.push(entry.path());
            }
        }
    }
    Ok(all_entries)
}

pub fn load_available_images(dir: PathBuf) -> Result<Vec<PathBuf>, io::Error> {
    load_media_files(&dir)
    // let images: Vec<PathBuf> = load_media_files(&dir)?
        // .iter()
        // .sorted()
        // .filter_map(|path| {
        //     let format = get_format(path)?;

        //     let file = std::fs::File::open(path).unwrap();
        //     // exif.get_field(tag, ifd_num)
        //     // println!("Read orientation {orientation:?}");
        //     // return orientation;
        //     // exif.

        //     // let Ok(meta) = Metadata::new_from_path(path) else {
        //     //     warn!("Image has no metadata, skipping: {:?}", path);
        //     //     return None;
        //     // };
        //     println!("path: {:?}", path);
        //     let embedded_thumbnail = if format == ImageFormat::Heif {
        //         let ctx = HeifContext::read_from_file(path.to_str().unwrap()).unwrap();
        //         let binding = ctx.top_level_image_handles();
        //         let handle = binding.first().unwrap();
        //         handle.number_of_thumbnails() > 0
        //     } else if format == ImageFormat::Video {
        //         false
        //     } else {
        //         has_thumbnail(file)
        //         // meta.get_preview_images().is_some()
        //     };
        //     // let mut orientation = Orientation::from_exif(meta.get_orientation() as u8)
        //     //     .unwrap_or(Orientation::NoTransforms);
        //     // let orientation = if format == ImageFormat::Video {
        //     //     let orientation = load_thumbnail_video(path).unwrap().orientation;
        //     //     debug!("video orientation: {:?}, {:?}", orientation, path);
        //     //     orientation
        //     // } else {
        //     //     Orientation::from_exif(exif_reader.unwrap().get_field(Tag::Orientation, exif::In(0)).map(|f| f.value.as_uint().unwrap().get(0).unwrap()).unwrap_or(0) as u8).unwrap()
        //     // };
        //     let hash = get_file_hash(path);
        //     // let tags = meta
        //     //     .get_tag_multiple_strings(EXIF_TAGLIST_TAG)
        //     //     .unwrap_or_default();
        //     let tags = vec![];
        //     let rating = read_rating_xmp(&path).unwrap_or(0);
        //     // match format {
        //     //     ImageFormat::Video => ,
        //     //     _ => meta.get_tag_numeric(EXIF_RATING_TAG),
        //     // };
        //     Some(ImageData {
        //         path: path.clone(),
        //         format,
        //         embedded_thumbnail,
        //         hash,
        //         rating,
        //         tags,
        //     })
        // })
        // .collect();
    // Ok(images)
}

fn has_thumbnail(exif: &nom_exif::Exif) -> bool {
    // let Ok(exif) = get_exif_data(file) else {
    //     return false;
    // };
    // let Ok(exif) = Reader::new().read_from_container(&mut std::io::BufReader::new(&file)) else {
    //     return false;
    // };
    // let comp = exif.get_field(Tag::Compression, exif::In(1)).is_some();
    let Some(compression_tag) = exif.get_by_ifd_tag_code(1, nom_exif::ExifTag::Compression.code())
    else {
        return false;
    };
    let comp = compression_tag.as_u16().unwrap() == 6;
    comp
}

// pub fn check_embedded_thumbnail(path: &PathBuf) -> bool {
//     rexiv2::Metadata::new_from_path(path).is_ok_and(|meta| meta.get_preview_images().is_some())
// }

// pub fn get_embedded_thumbnail(image: &ImageData) -> Option<Vec<u8>> {
//     let meta = rexiv2::Metadata::new_from_path(&image.path).ok()?;
//     meta.get_preview_images()?
//         .first()
//         .and_then(|preview| preview.get_data().ok())
// }

fn load_jpg_full(path: &Path) -> ImflowImageBuffer {
    let file = std::fs::File::open(path).unwrap();
    let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);

    let file_bytes = read(path).unwrap();
    let mut decoder = JpegDecoder::new(&file_bytes);
    decoder.set_options(options);

    decoder
        .decode_headers()
        .map_err(|e| MediaLoadError::Decoding(format!("Failed to decode JPEG headers: {}", e))).unwrap();
    let info = decoder
        .info()
        .ok_or_else(|| MediaLoadError::Decoding("Failed to read JPEG info".to_string())).unwrap();

    let width = info.width as usize;
    let height = info.height as usize;
    let mut buffer: Vec<u8> = vec![0; width * height * 4];
    decoder
        .decode_into(buffer.as_mut_slice())
        .map_err(|e| MediaLoadError::Decoding(e.to_string())).unwrap();

    // let exif_data = decoder.exif().unwrap();
    let exif = get_exif_data(file.try_clone().unwrap()).unwrap();

    let orientation = Orientation::from_exif(
        exif.get_by_ifd_tag_code(0, nom_exif::ExifTag::Orientation.code()).unwrap().as_u16().unwrap() as u8
    )
    .unwrap();

    let rgba_buffer = vec_u8_to_u32(buffer);

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer,
        orientation,
    }
}

pub fn load_file_data(path: &Path) -> Option<(ImageData, Option<ImflowImageBuffer>)> {
    let format = get_format(path)?;
    let mut file = std::fs::File::open(path).unwrap();
    let hash = get_file_hash(&file);
    debug!("Loading file data from: {:?}", path);
    let (data, thumbnail) = match format {
        ImageFormat::Jpg => {
            // let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);

            // let file_bytes = read(path).unwrap();
            // let mut decoder = JpegDecoder::new(&file_bytes);
            // decoder.set_options(options);

            // decoder
            //     .decode_headers()
            //     .map_err(|e| MediaLoadError::Decoding(format!("Failed to decode JPEG headers: {}", e))).unwrap();
            // let info = decoder
            //     .info()
            //     .ok_or_else(|| MediaLoadError::Decoding("Failed to read JPEG info".to_string())).unwrap();

            // let width = info.width as usize;
            // let height = info.height as usize;
            // let mut buffer: Vec<u8> = vec![0; width * height * 4];
            // decoder
            //     .decode_into(buffer.as_mut_slice())
            //     .map_err(|e| MediaLoadError::Decoding(e.to_string())).unwrap();

            // let exif_data = decoder.exif().unwrap();
            file.seek(io::SeekFrom::Start(0)).unwrap();
            let exif = get_exif_data(file.try_clone().unwrap()).unwrap();

            // let orientation = Orientation::from_exif(
            //     exif.get_by_ifd_tag_code(0, nom_exif::ExifTag::Orientation.code()).unwrap().as_u8().unwrap()
            // )
            // .unwrap();

            // let rgba_buffer = vec_u8_to_u32(buffer);

            // let image = ImflowImageBuffer {
            //     width,
            //     height,
            //     rgba_buffer,
            //     orientation,
            // };

            let thumbnail = if has_thumbnail(&exif) {
                Some(load_thumbnail_exif(exif, file).unwrap())
            } else {
                Some(load_thumbnail_full(path))
            };

            let rating = read_rating_xmp(&path).unwrap_or(0);
            let data = ImageData{
                path: path.to_path_buf(),
                format,
                embedded_thumbnail: thumbnail.is_some(),
                hash,
                rating,
                tags: vec![],
            };

            (data, thumbnail)
        },
        ImageFormat::Jxl => {
            todo!();
        },
        ImageFormat::Heif => {
            let ctx = HeifContext::read_from_file(path.to_str().unwrap()).unwrap();
            let binding = ctx.top_level_image_handles();
            let main_handle = binding.first().unwrap();
            let thumbnail = if main_handle.number_of_thumbnails() > 0 {
                Some(load_thumbnail_heif(main_handle))
            } else {
                Some(load_heif(path, true)) // TODO: optimize
            };
            let all_metadata = main_handle.all_metadata();
            let mut rating = 0;
            if let Some(xmp_raw) = all_metadata.iter().find(|m| m.content_type == "application/rdf+xml") {
                rating = read_rating_from_raw_xmp(&xmp_raw.raw_data).unwrap_or(0);
            }
            let data = ImageData{
                path: path.to_path_buf(),
                format,
                embedded_thumbnail: thumbnail.is_some(),
                hash,
                rating,
                tags: vec![],
            };
            (data, thumbnail)
        },
        ImageFormat::Video => {
            let rating = read_rating_xmp(&path).unwrap_or(0);
            let thumbnail = load_thumbnail_video(&path);
            let data = ImageData{
                path: path.to_path_buf(),
                format,
                embedded_thumbnail: thumbnail.is_some(),
                hash,
                rating,
                tags: vec![],
            };
            (data, thumbnail)
        },
    };
    Some((data, thumbnail))
}

// pub fn load_thumbnail(path: &ImageData) -> ImflowImageBuffer {
//     let cache_path = path.get_cache_path();
//     let mut buffer: Option<Vec<u8>> = None;
//     if cache_path.exists() {
//         let read_bytes = fs::read(&cache_path).unwrap();
//         if read_bytes.is_empty() {
//             buffer = Some(read_bytes);
//         }
//     }
//     if let Some(bytes) = buffer {
//         debug!("Loading thumbnail for: {:?}", path.path);
//         let width = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
//         let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
//         let orientation =
//             Orientation::from_exif(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as u8)
//                 .unwrap_or(Orientation::NoTransforms);
//         let (ptr, len, cap) = bytes.into_raw_parts();
//         assert!(ptr.align_offset(4) == 0);
//         let buffer_u32 = unsafe {
//             Vec::from_raw_parts(
//                 (ptr as usize + 12) as *mut u32,
//                 (len - 12) / 4,
//                 (cap - 12) / 4,
//             )
//         };

//         assert_eq!(width * height, buffer_u32.len());

//         return ImflowImageBuffer {
//             width,
//             height,
//             rgba_buffer: buffer_u32,
//             orientation,
//         };
//     }
//     let thumbnail = match path.format {
//         ImageFormat::Heif => load_heif(path, true),
//         ImageFormat::Video => load_thumbnail_video(&path.path).unwrap(),
//         _ => load_thumbnail_exif(path).unwrap_or_else(|| {
//             println!("LOADING FULL {:?}", path.path);
//             load_thumbnail_full(path)
//         }),
//     };

//     save_thumbnail(&cache_path, thumbnail.clone());
//     thumbnail
// }

fn get_exif_data<R: Read + Seek>(source: R) -> nom_exif::Result<nom_exif::Exif> {
    let mut parser = nom_exif::MediaParser::new();
    let ms = nom_exif::MediaSource::seekable(source)?;
    assert!(ms.has_exif());

    let iter: nom_exif::ExifIter = parser.parse(ms)?;
    let exif: nom_exif::Exif = iter.into();
    Ok(exif)
}

fn load_thumbnail_heif(handle: &libheif_rs::ImageHandle) -> ImflowImageBuffer {
    let lib_heif = LibHeif::new();
    let thumbnail_count = handle.number_of_thumbnails() as u32;
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

    let thumbnail = lib_heif
        .decode(handle, libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba), None)
        .unwrap()
        .scale(new_width, new_height, None)
        .unwrap();

    let planes = thumbnail.planes();
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
        orientation: Orientation::NoTransforms,
    }
}

fn load_image_heif(handle: &libheif_rs::ImageHandle) -> ImflowImageBuffer {
    let lib_heif = LibHeif::new();

    let thumbnail = lib_heif
        .decode(&handle, libheif_rs::ColorSpace::Rgb(RgbChroma::Rgba), None)
        .unwrap();

    let planes = thumbnail.planes();
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
        orientation: Orientation::NoTransforms,
    }
}

pub fn load_thumbnail_exif<R: Read + Seek>(exif: nom_exif::Exif, mut file: R) -> Option<ImflowImageBuffer> {
    // if path.embedded_thumbnail {
    //     println!("Loading file: {:?}", path.path);
        // let mut file = File::open(&path.path).unwrap();

        // let exif = get_exif_data(file.try_clone().unwrap()).unwrap();
        let thumbnail_offset = exif
            .get_by_ifd_tag_code(1, nom_exif::ExifTag::ThumbnailOffset.code())
            .unwrap()
            .as_u32()
            .unwrap() as u64;
        let thumbnail_length = exif
            .get_by_ifd_tag_code(1, nom_exif::ExifTag::ThumbnailLength.code())
            .unwrap()
            .as_u32()
            .unwrap() as u64;
        let thumbnail_orientation = exif
            .get_by_ifd_tag_code(1, nom_exif::ExifTag::Orientation.code())
            .map(|e| e.as_u16().unwrap() as u8);
        // TODO: Support other formats
        let compression = exif
            .get_by_ifd_tag_code(1, nom_exif::ExifTag::Compression.code())
            .unwrap()
            .as_u16()
            .unwrap();
        assert_eq!(compression, 6);

        file.seek(io::SeekFrom::Start(thumbnail_offset)).unwrap();
        let mut tmp_buf = [0u8; 128];
        file.read_exact(tmp_buf.as_mut_slice()).unwrap();
        println!("{:?}", tmp_buf);
        const JPG_MAGIC: &[u8; 3] = &[0xff, 0xd8, 0xff];
        let header_offset = tmp_buf.windows(3).position(|p| p == JPG_MAGIC).unwrap() as u64;
        println!("offset: {:?}", header_offset);
        file.seek(io::SeekFrom::Start(thumbnail_offset + header_offset))
            .unwrap();

        let mut buf = vec![0u8; (thumbnail_length) as usize];
        // file.seek_relative(header_offset as i64).unwrap();
        file.read_exact(buf.as_mut_slice()).unwrap();
        println!(
            "off {:#X}: {:#X} {:#X} {:#X}",
            thumbnail_offset, buf[0], buf[1], buf[2]
        );
        let mut decoder = image::ImageReader::new(Cursor::new(buf))
            .with_guessed_format()
            .unwrap();
        decoder.set_format(image::ImageFormat::Jpeg);
        let image = decoder.decode().unwrap();
        let width: usize = image.width() as usize;
        let height: usize = image.height() as usize;
        let rgba_buffer = image_to_rgba_buffer(image);
        let orientation = thumbnail_orientation
            .map(|o| Orientation::from_exif(o).unwrap())
            .unwrap_or(Orientation::NoTransforms);

        Some(ImflowImageBuffer {
            width,
            height,
            rgba_buffer,
            orientation,
        })

        // let Ok(exif) = Reader::new().read_from_container(&mut std::io::BufReader::new(&path.path)) else {
        //     return false;
        // };
        // nom_ex
        // let comp = exif.buf();
        // return None;
        // todo!()
        // let decoder = image::ImageReader::new(Cursor::new(thumbnail))
        //     .with_guessed_format()
        //     .unwrap();
        // let image = decoder.decode().unwrap();
        // let orientation = path.orientation;

        // let width = image.width();
        // let height = image.height();
        // // TODO: extract from image
        // let ratio_image = 1.5;
        // let ratio_thumbnail = width as f32 / height as f32;
        // let crop = ratio_thumbnail / ratio_image;
        // let start = ((0.5 - (crop / 2.0)) * height as f32).round();
        // let cropped_height = (height as f32 * crop) as u32;
        // let mut image = image.crop_imm(0, start as u32, width, cropped_height);

        // image.apply_orientation(orientation);
        // let width: usize = image.width() as usize;
        // let height: usize = image.height() as usize;
        // let rgba_buffer = image_to_rgba_buffer(image);

        // Some(ImflowImageBuffer {
        //     width,
        //     height,
        //     rgba_buffer,
        //     orientation: Orientation::NoTransforms,
        // })
}

pub fn load_thumbnail_video(path: &Path) -> Option<ImflowImageBuffer> {
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
    debug!("frame wh: {:?}, {:?} {}\n", key_frame.width(), key_frame.height(), round_to_4_multiple(decoder.width() / 4));

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
    debug!("smaller frame wh: {:?}, {:?}\n", rgba_frame.width(), rgba_frame.height());
    Some(ImflowImageBuffer {
        width: rgba_frame.width() as usize,
        height: rgba_frame.height() as usize,
        rgba_buffer: buffer.to_vec(),
        orientation: orientation.unwrap_or(Orientation::NoTransforms),
    })
}

pub fn load_thumbnail_full(path: &Path) -> ImflowImageBuffer {
    let mut decoder = ImageReader::open(path)
        .unwrap()
        .into_decoder()
        .unwrap();
    let orientation = decoder.orientation().unwrap();
    let image = DynamicImage::from_decoder(decoder).unwrap();

    let image = image.resize_to_fill(720, 720, FilterType::Nearest);

    let width = image.width() as usize;
    let height = image.height() as usize;
    let start = std::time::Instant::now();
    let buffer = image_to_rgba_buffer(image);
    debug!("Elapsed: {:?}", start.elapsed());

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: buffer,
        orientation,
    }
}

pub fn load_heif(path: &Path, resize: bool) -> ImflowImageBuffer {
    let lib_heif = LibHeif::new();
    let ctx = HeifContext::read_from_file(path.to_str().unwrap()).unwrap();
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
        let exif_reader = Reader::new();
        if let Ok(exif) = handle.metadata(meta_ids[0])
            && let Ok(metadata) = exif_reader.read_raw(exif)
        {
            orientation = Orientation::from_exif(
                metadata
                    .get_field(Tag::Orientation, exif::In(0))
                    .map(|f| f.value.as_uint().unwrap().get(0).unwrap())
                    .unwrap_or(0) as u8,
            )
            .unwrap();
            println!("Read orientation {orientation:?}");
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
        width = round_to_4_multiple((width as f32 / scale) as u32) as usize;
        height = round_to_4_multiple((height as f32 / scale) as u32) as usize;
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

pub fn get_file_hash(mut file: &File) -> GenericArray<u8, U32> {
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
    file.write_all(&(image.height as u32).to_le_bytes())
        .unwrap();
    file.write_all(&(image.orientation.to_exif() as u32).to_le_bytes())
        .unwrap();
    file.write_all(&u8_buffer).unwrap();
}
