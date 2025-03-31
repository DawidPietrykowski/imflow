#![allow(unused)]

use std::iter;
use std::time::Duration;

use criterion::{AxisScale, BenchmarkId, PlotConfiguration};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use imflow::image::{
    load_available_images, load_image, load_thumbnail_exif, load_thumbnail_full
};
const PATH: &str = "test_images";

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

criterion_group!(benches, thumbnail_load_benchmark);
criterion_main!(benches);
