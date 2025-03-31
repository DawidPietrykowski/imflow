use iced::widget::image::Handle;
use image::DynamicImage;
use image::imageops::FilterType;
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
    pub argb_buffer: Vec<u32>,
}

pub fn create_iced_handle(width: u32, height: u32, rgba: Vec<u8>) -> Handle {
    Handle::from_rgba(width, height, rgba)
}

pub fn load_image(path: PathBuf) -> ImflowImageBuffer {
    let total_start = Instant::now();

    let file = read(path).unwrap();
    let mut decoder = JpegDecoder::new(&file);
    let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::BGRA);
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

    ImflowImageBuffer {
        width,
        height,
        argb_buffer: buffer_u32,
    }
}

pub fn image_to_argb_buffer(img: DynamicImage) -> Vec<u32> {
    let flat = img.into_rgba8();
    let buf = flat.as_raw();

    buf.chunks_exact(4).map(|rgba| {
        let r = rgba[0] as u32;
        let g = rgba[1] as u32;
        let b = rgba[2] as u32;
        r << 16 | g << 8 | b
    }).collect()
}

pub fn load_available_images(dir: PathBuf) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|f| f.unwrap().path())
        .filter(|f| f.extension().unwrap().to_ascii_lowercase() == "jpg")
        .collect();
    files.sort();
    files
}

pub fn get_embedded_thumbnail(path: PathBuf) -> Option<Vec<u8>> {
    let meta = rexiv2::Metadata::new_from_path(path);
    match meta {
        Ok(meta) => {
            for preview in meta.get_preview_images().unwrap() {
                return Some(preview.get_data().unwrap());
            }
            None
        }
        Err(_) => None,
    }
}

pub fn load_thumbnail(path: &PathBuf) -> ImflowImageBuffer {
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
            let mut flat = image.into_rgba8().into_raw();
            let mut buffer: Vec<u32> = vec![0; width * height];

            for (rgba, argb) in flat.chunks_mut(4).zip(buffer.iter_mut()) {
                let r = rgba[0] as u32;
                let g = rgba[1] as u32;
                let b = rgba[2] as u32;
                *argb = r << 16 | g << 8 | b;
            }

            Some(ImflowImageBuffer {
                width,
                height,
                argb_buffer: buffer,
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
    let buffer = image_to_argb_buffer(image);

    ImflowImageBuffer {
        width,
        height,
        argb_buffer: buffer,
    }
}
