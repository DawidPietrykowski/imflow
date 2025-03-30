#![allow(unused)]

use std::iter;
use std::time::Duration;

use criterion::BenchmarkId;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use imflow::image::{load_available_images, load_image_argb, load_image_argb_imagers, load_thumbnail, Approach};
const PATH: &str = "test_images";

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("image_decode");

    group
        .sample_size(10) // Reduce number of samples (default is 100)
        .measurement_time(Duration::from_millis(500)) // Reduce measurement time (default is 5 seconds)
        .warm_up_time(Duration::from_millis(200)); // Reduce warm-up time (default is 3 seconds)

    let images = load_available_images(PATH.into());
    for image in images.iter() {
        let image_name = image.to_str().unwrap();
        
        // Benchmark zune for this image
        group.bench_with_input(
            format!("{}/zune", image_name),
            image,
            |b, image| {
                b.iter(|| load_image_argb(image.clone().into()));
            },
        );

        // Benchmark image-rs for the same image
        group.bench_with_input(
            format!("{}/image-rs", image_name),
            image,
            |b, image| {
                b.iter(|| load_image_argb_imagers(image.clone().into()));
            },
        );
    }
    
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
