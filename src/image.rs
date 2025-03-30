use iced::widget::image::Handle;
use iced::widget::image::Image as IcedImage;
// use image::codecs::jpeg::JpegDecoder;
// use image::codecs::jpeg::JpegDecoder;
use image::DynamicImage;
use image::ImageReader;
use itertools::Itertools;
use memmap2::Mmap;
use zune_image::codecs::jpeg::JpegDecoder;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions;
use zune_image::image::Image as ZuneImage;

use std::fs;
use std::fs::File;
use std::fs::read;
use std::io;
use std::io::Read;
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Instant;

pub enum Approach {
    Mmap,
    Path,
    ImageRs,
    Iced,
    ImageRsPath,
}

pub fn convert_zune_rgb_to_rgba(rgb_data: Vec<Vec<u8>>) -> Vec<u8> {
    let r_channel = &rgb_data[0];

    let num_pixels = r_channel.len() / 3;
    let mut rgba_data: Vec<u8> = Vec::with_capacity(num_pixels * 4);

    for i in 0..num_pixels {
        rgba_data.push(r_channel[i * 3]);
        rgba_data.push(r_channel[i * 3 + 1]);
        rgba_data.push(r_channel[i * 3 + 2]);
        rgba_data.push(255); // Fully opaque Alpha value
    }

    rgba_data
}

pub fn map_file(path: &str) -> io::Result<Mmap> {
    let file = File::open(path)?;
    unsafe { Mmap::map(&file) }
}

pub fn map_file_path(path: PathBuf) -> io::Result<Mmap> {
    let file = File::open(path)?;
    unsafe { Mmap::map(&file) }
}

pub fn read_zune_image(mmap: &[u8]) -> Result<ZuneImage, String> {
    ZuneImage::read(mmap, DecoderOptions::new_fast()).map_err(|e| e.to_string())
}

pub fn read_zune_image_path(path: &str) -> ZuneImage {
    // let file = File::open(path).unwrap();
    // let file_contents = read(path).unwrap();
    // let mmap = map_file_path(path.into()).unwrap();
    // let decoder = JpegDecoder::new(&file_contents);
    // decoder.decode_into()
    // ZuneImage::read(mmap, DecoderOptions::new_fast()).map_err(|e| e.to_string())
    zune_image::image::Image::open_with_options(path, DecoderOptions::new_fast()).unwrap()
}

pub fn flatten_zune_image(img: &ZuneImage) -> Vec<Vec<u8>> {
    img.flatten_to_u8()
}

pub fn flatten_image_image(img: DynamicImage) -> Vec<u8> {
    img.into_rgba8().into_raw()
}

pub fn create_iced_handle(width: u32, height: u32, rgba: Vec<u8>) -> Handle {
    Handle::from_rgba(width, height, rgba)
}

pub fn load_thumbnail(
    path: &str,
    approach: Approach,
) -> Result<iced::widget::image::Handle, String> {
    match approach {
        Approach::Mmap => {
            let mmap = map_file(path).unwrap();
            println!("mapped file");
            let img = read_zune_image(mmap.deref())?;
            let width = img.dimensions().0 as u32;
            let height = img.dimensions().1 as u32;
            println!("loaded");
            let flat = flatten_zune_image(&img);
            println!("flattened");
            let rgba = convert_zune_rgb_to_rgba(flat);
            println!("rgbad");
            let conv = create_iced_handle(width, height, rgba);
            println!("iced");

            Ok(conv)
        }
        Approach::Path => {
            let img = read_zune_image_path(path);
            let width = img.dimensions().0 as u32;
            let height = img.dimensions().1 as u32;
            println!("loaded");
            let flat = flatten_zune_image(&img);
            println!("flattened");
            let rgba = convert_zune_rgb_to_rgba(flat);
            println!("rgbad");
            let conv = create_iced_handle(width, height, rgba);
            println!("iced");

            Ok(conv)
        }
        Approach::ImageRs => {
            let mmap = map_file(path).unwrap();
            let img = image::load_from_memory(mmap.deref()).map_err(|e| e.to_string())?;
            let width = img.width();
            let height = img.height();
            println!("loaded");
            let rgba = flatten_image_image(img);
            println!("rgbad");
            let conv = create_iced_handle(width, height, rgba);
            println!("iced");

            Ok(conv)
        }
        Approach::ImageRsPath => {
            let img = ImageReader::open(path).unwrap().decode().unwrap();
            let width = img.width();
            let height = img.height();
            println!("loaded");
            let rgba = flatten_image_image(img);
            println!("rgbad");
            let conv = create_iced_handle(width, height, rgba);
            println!("iced");

            Ok(conv)
        }
        Approach::Iced => Ok(Handle::from_path(path)),
    }
}

pub fn load_image_argb(path: PathBuf) -> ImflowImageBuffer {
    let total_start = Instant::now();

    // Stage 1: Memory map the file
    let stage1_start = Instant::now();
    let mmap = map_file_path(path.clone()).unwrap();
    let stage1_time = stage1_start.elapsed();
    // println!("File mapping took: {:?}", stage1_time);

    // let file = File::open(path).unwrap();
    let file_contents = read(path).unwrap();
    // let mmap = map_file_path(path.into()).unwrap();

    let mut decoder = JpegDecoder::new(&file_contents);
    let options = DecoderOptions::new_fast()
        .jpeg_set_max_scans(5)
        .jpeg_set_out_colorspace(zune_image::codecs::qoi::zune_core::colorspace::ColorSpace::BGRA);
    decoder.set_options(options);
    decoder.decode_headers().unwrap();
    let info = decoder.info().unwrap();
    let width = info.width as usize;
    let height = info.height as usize;
    println!("{} x {}", width, height);
    let mut buffer2: Vec<u8> = vec![0; width * height * 4];
    decoder.decode_into(buffer2.as_mut_slice());

    // Stage 2: Read the image
    // let stage2_start = Instant::now();
    // let img = read_zune_image(mmap.deref()).unwrap();
    // let width = img.dimensions().0;
    // let height = img.dimensions().1;
    // let stage2_time = stage2_start.elapsed();
    // println!("Image decoding took: {:?}", stage2_time);

    // Stage 3: Flatten the image
    // let stage3_start = Instant::now();
    // let flat = &mut flatten_zune_image(&img)[0];
    // let stage3_time = stage3_start.elapsed();
    // println!("Image flattening took: {:?}", stage3_time);

    // Stage 4: Convert to ARGB format
    let stage4_start = Instant::now();
    // let mut buffer: Vec<u32> = vec![0; width * height];

    // for (rgba, argb) in buffer2.chunks_mut(4).zip(buffer.iter_mut()) {
    //     let r = rgba[0] as u32;
    //     let g = rgba[1] as u32;
    //     let b = rgba[2] as u32;
    //     *argb = r << 16 | g << 8 | b;
    // }
    let buffer: &[u32] =
        unsafe { std::slice::from_raw_parts(buffer2.as_ptr() as *const u32, buffer2.len() / 4) };
    let stage4_time = stage4_start.elapsed();
    println!("RGBA to ARGB conversion took: {:?}", stage4_time);

    // Total time
    let total_time = total_start.elapsed();
    println!("Total loading time: {:?}", total_time);

    ImflowImageBuffer {
        width,
        height,
        argb_buffer: buffer.to_vec(),
    }
}

pub struct ImflowImageBuffer {
    pub width: usize,
    pub height: usize,
    pub argb_buffer: Vec<u32>,
}

pub fn load_image_argb_imagers(path: PathBuf) -> ImflowImageBuffer {
    let total_start = Instant::now();

    // Stage 1: Memory map the file
    let stage1_start = Instant::now();
    let mmap = map_file_path(path).unwrap();
    let stage1_time = stage1_start.elapsed();
    // println!("File mapping took: {:?}", stage1_time);

    // Stage 2: Read the image
    let stage2_start = Instant::now();
    let img = image::load_from_memory(mmap.deref())
        .map_err(|e| e.to_string())
        .unwrap();
    let width = img.width() as usize;
    let height = img.height() as usize;
    let stage2_time = stage2_start.elapsed();
    // println!("Image decoding took: {:?}", stage2_time);

    // Stage 3: Flatten the image
    let stage3_start = Instant::now();
    let mut flat = img.into_rgba8().into_raw();
    let stage3_time = stage3_start.elapsed();
    // println!("Image flattening took: {:?}", stage3_time);

    // Stage 4: Convert to ARGB format
    let stage4_start = Instant::now();
    let mut buffer: Vec<u32> = vec![0; width * height];

    for (rgba, argb) in flat.chunks_mut(4).zip(buffer.iter_mut()) {
        let r = rgba[0] as u32;
        let g = rgba[1] as u32;
        let b = rgba[2] as u32;
        *argb = r << 16 | g << 8 | b;
    }
    let stage4_time = stage4_start.elapsed();
    // println!("RGBA to ARGB conversion took: {:?}", stage4_time);

    // Total time
    let total_time = total_start.elapsed();
    println!("Total loading time: {:?}", total_time);

    ImflowImageBuffer {
        width,
        height,
        argb_buffer: buffer,
    }
}
pub fn load_available_images(dir: PathBuf) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|f| f.unwrap().path())
        .collect();
    files.sort();
    files
}

pub fn get_embedded_thumbnail(path: PathBuf) -> Option<Vec<u8>> {
    let meta = rexiv2::Metadata::new_from_path(path);
    match meta {
        Ok(meta) => {
            meta.get_thumbnail().map(|v| v.to_vec())
        }
        Err(e) => None,
    }
    // let file = std::fs::File::open(path).ok()?;
    // let exif = Reader::new().read_from_container(&mut std::io::BufReader::new(file)).ok()?;
    // exif.get_thumbnail()
}
