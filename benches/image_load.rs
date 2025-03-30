#![allow(unused)]

use std::any::Any;
use std::fs::{File, read};
use std::io::{BufReader, Cursor};
use std::iter;
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Duration;

use criterion::{AxisScale, BenchmarkId, PlotConfiguration};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use image::codecs::jpeg::JpegDecoder;
use image::metadata::Orientation;
use image::{DynamicImage, ImageResult, RgbaImage};
use imflow::image::{
    ImflowImageBuffer, get_orientation, get_rating, image_to_rgba_buffer, load_available_images,
    load_image, load_thumbnail_exif, load_thumbnail_full,
};
use jpegxl_rs::Endianness;
use jpegxl_rs::decode::{Data, PixelFormat, Pixels};
use jpegxl_rs::decoder_builder;
use zune_image::codecs::jpeg::JpegDecoder as ZuneJpegDecoder;
use zune_image::codecs::qoi::zune_core::colorspace::ColorSpace;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions;
const PATH: &str = "test_images";

/// Create a new decoder that decodes from the stream ```r```
// pub fn new(r: R) -> ImageResult<JpegDecoder<R>> {
//     let mut input = Vec::new();
//     let mut r = r;
//     r.read_to_end(&mut input)?;
//     let options = DecoderOptions::default()
//         .set_strict_mode(false)
//         .set_max_width(usize::MAX)
//         .set_max_height(usize::MAX);
//     let mut decoder = ZuneJpegDecoder::new_with_options(input.as_slice(), options);
//     decoder.decode_headers().map_err(ImageError::from_jpeg)?;
//     // now that we've decoded the headers we can `.unwrap()`
//     // all these functions that only fail if called before decoding the headers
//     let (width, height) = decoder.dimensions().unwrap();
//     // JPEG can only express dimensions up to 65535x65535, so this conversion cannot fail
//     let width: u16 = width.try_into().unwrap();
//     let height: u16 = height.try_into().unwrap();
//     let orig_color_space = decoder.get_output_colorspace().unwrap();
//     // Limits are disabled by default in the constructor for all decoders
//     let limits = Limits::no_limits();
//     Ok(JpegDecoder {
//         input,
//         orig_color_space,
//         width,
//         height,
//         limits,
//         orientation: None,
//         phantom: PhantomData,
//     })
// }
// pub fn full_load_benchmark(c: &mut Criterion) {
//     let mut group = c.benchmark_group("image_decode");

//     group
//         .sample_size(10)
//         .measurement_time(Duration::from_millis(500))
//         .warm_up_time(Duration::from_millis(200));

//     let images = load_available_images(PATH.into());
//     for image in images.iter() {
//         let image_name = image.to_str().unwrap();

//         group.bench_with_input(format!("{}/zune", image_name), image, |b, image| {
//             b.iter(|| load_image_argb(image.clone().into()));
//         });

//         group.bench_with_input(format!("{}/image-rs", image_name), image, |b, image| {
//             b.iter(|| load_image_argb_imagers(image.clone().into()));
//         });
//     }

//     group.finish();
// }
fn load_a(path: &PathBuf) -> ImflowImageBuffer {
    let file = read(path.clone()).unwrap();
    let mut decoder = ZuneJpegDecoder::new(&file);
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

    // let total_time = total_start.elapsed();
    // println!("Total loading time: {:?}", total_time);

    let rating = get_rating(path);

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: buffer_u32,
        rating,
    }
}

fn load_b(path: &PathBuf) -> ImflowImageBuffer {
    let file = read(path.clone()).unwrap();
    let mut decoder = ZuneJpegDecoder::new(&file);
    let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);
    decoder.set_options(options);

    decoder.decode_headers().unwrap();
    let info = decoder.info().unwrap();
    let width = info.width as usize;
    let height = info.height as usize;

    let mut buffer: Vec<u8> = vec![0; width * height * 4];
    decoder.decode_into(buffer.as_mut_slice()).unwrap();

    let image = RgbaImage::from_raw(width as u32, height as u32, buffer).unwrap();
    let orientation = Orientation::from_exif(get_orientation(path)).unwrap();
    let mut dynamic_image = DynamicImage::from(image);
    dynamic_image.apply_orientation(orientation);

    let rating = get_rating(path);

    let mut buffer = dynamic_image.to_rgba8();
    let buffer_u32 = unsafe {
        Vec::from_raw_parts(
            buffer.as_mut_ptr() as *mut u32,
            buffer.len() / 4,
            buffer.len() / 4,
        )
    };
    std::mem::forget(buffer);

    ImflowImageBuffer {
        width,
        height,
        rgba_buffer: buffer_u32,
        rating,
    }
}

fn load_jxl_single(path: &PathBuf) -> (jpegxl_rs::decode::Metadata, Vec<u8>) {
    let file = read(path).unwrap();
    use jpegxl_rs::ThreadsRunner;
    let runner = ThreadsRunner::default();
    let decoder = decoder_builder()
        // .parallel_runner(&runner)
        .pixel_format(PixelFormat {
            num_channels: 4,
            endianness: Endianness::Big,
            align: 8,
        })
        .build()
        .unwrap();

    decoder.decode_with::<u8>(&file).unwrap()
    // buffer = data;
    // width = metadata.width as usize;
    // height = metadata.height as usize;
}

fn load_jxl_multi(path: &PathBuf) -> (jpegxl_rs::decode::Metadata, Vec<u8>) {
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

    decoder.decode_with::<u8>(&file).unwrap()
    // buffer = data;
    // width = metadata.width as usize;
    // height = metadata.height as usize;
}

// fn load_b(path: &PathBuf) -> ImflowImageBuffer {
//     println!("path: {:?}", path);
//     // let file = read(path.clone()).unwrap();
//     let file = BufReader::new(File::open(path).unwrap());
//     let decoder = image::ImageReader::new(file).unwrap();
//     let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGBA);
//     decoder.set_options(options);
//     let image = reader
//         .with_guessed_format()
//         .unwrap()
//         .decode()
//         .unwrap();
//     let width = image.width() as usize;
//     let height = image.height() as usize;
//     // let buffer = image_to_rgba_buffer(image);
//     let im = RgbaImage::from_raw(width, height, image.as_rgba8()).unwrap();
//     let rating = get_rating(path.into());

//     ImflowImageBuffer {
//         width,
//         height,
//         rgba_buffer: buffer,
//         rating,
//     }
// }

pub fn thumbnail_load_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("thumbnail");

    group
        .sample_size(10)
        .measurement_time(Duration::from_millis(500))
        .warm_up_time(Duration::from_millis(200));

    let images = load_available_images(PATH.into());
    group.bench_function("exif", |b| {
        for image in images.iter().take(10) {
            b.iter(|| load_thumbnail_exif(image));
        }
    });
    group.bench_function("full", |b| {
        for image in images.iter().take(10) {
            b.iter(|| load_thumbnail_full(image));
        }
    });

    group.finish();
}

pub fn file_load_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("image_load");

    group
        .sample_size(10)
        .measurement_time(Duration::from_millis(500))
        .warm_up_time(Duration::from_millis(200));

    let images = load_available_images(PATH.into());
    group.bench_function("zune_jpeg", |b| {
        for image in images.iter().take(10) {
            b.iter(|| load_a(image));
        }
    });
    group.bench_function("image_rs", |b| {
        for image in images.iter().take(10) {
            b.iter(|| load_b(image));
        }
    });

    group.finish();
}

pub fn jxl_multithreading_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("jxl_multithreading");

    group
        .sample_size(10)
        .measurement_time(Duration::from_millis(500))
        .warm_up_time(Duration::from_millis(200));

    let images = load_available_images("./test_images/jxl".into());
    group.bench_function("single", |b| {
        for image in images.iter().take(10) {
            b.iter(|| load_jxl_single(image));
        }
    });
    group.bench_function("multi", |b| {
        for image in images.iter().take(10) {
            b.iter(|| load_jxl_multi(image));
        }
    });

    group.finish();
}
// criterion_group!(benches, thumbnail_load_benchmark);
// criterion_group!(benches, file_load_benchmark);
criterion_group!(benches, jxl_multithreading_benchmark);
criterion_main!(benches);
