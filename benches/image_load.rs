#![allow(unused)]

use std::iter;

use criterion::BenchmarkId;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use imflow::image::{Approach, load_thumbnail};
const PATH: &str = "test_images/20240811-194516_DSC02274.JPG";

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("from_elem");
    group.bench_function("mmap", |b| b.iter(|| load_thumbnail(PATH, Approach::Mmap)));
    group.bench_function("path", |b| b.iter(|| load_thumbnail(PATH, Approach::Path)));
    group.bench_function("iced", |b| b.iter(|| load_thumbnail(PATH, Approach::Iced)));
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
