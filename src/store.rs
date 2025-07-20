use crate::image::{ImageData, ImageFormat, load_thumbnail};
use crate::image::{ImflowImageBuffer, load_available_images, load_image};
use crossbeam_channel::{Receiver, Sender, unbounded};
use exiftool::ExifTool;
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::collections::HashSet;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Instant;
use threadpool::ThreadPool;

const PRELOAD_NEXT_IMAGE_N: usize = 15;
const MAX_LOADED_IMAGES: usize = 450;

#[derive(Clone)]
pub struct FileFilters {
    pub rating: [bool; 6],
    pub name: String,
    pub file_format: HashMap<ImageFormat, bool>,
}

impl Default for FileFilters {
    fn default() -> Self {
        let mut formats = HashMap::new();
        formats.insert(ImageFormat::Jpg, true);
        formats.insert(ImageFormat::Jxl, true);
        formats.insert(ImageFormat::Heif, true);
        FileFilters {
            rating: [true; 6],
            name: "".to_string(),
            file_format: formats,
        }
    }
}

pub struct ImageStore {
    pub current_image_id: usize,
    pub(crate) loaded_images: FxHashMap<ImageData, ImflowImageBuffer>,
    pub(crate) loaded_images_thumbnails: FxHashMap<ImageData, ImflowImageBuffer>,
    pub available_images: Vec<ImageData>,
    pub current_image_path: ImageData,
    pub image_changed: bool,
    pub(crate) pool: ThreadPool,
    pub(crate) loader_rx: Receiver<(ImageData, ImflowImageBuffer)>,
    pub(crate) loader_tx: Sender<(ImageData, ImflowImageBuffer)>,
    pub(crate) currently_loading: HashSet<ImageData>,
    pub load_times: VecDeque<ImageData>,
}

impl ImageStore {
    pub fn new(path: PathBuf) -> Self {
        let current_image_id: usize = 0;
        let available_images = load_available_images(path);
        let new_path = available_images[0].clone();

        let (loader_tx, loader_rx) = unbounded();

        let pool = ThreadPool::new(32);

        let currently_loading = HashSet::new();

        // let first_image_path = available_images[0].clone();
        // let first_image_thread = std::thread::spawn(move || {
        //     let image = load_image(&first_image_path);
        //     (first_image_path, image)
        // });

        let total_start = Instant::now();
        let (sender, receiver) = unbounded();
        available_images
            .par_iter()
            .for_each_with(sender, |s, path| {
                // if path.embedded_thumbnail {
                let buf = load_thumbnail(path);
                s.send((path.clone(), buf)).unwrap();
                // }
            });

        let mut loaded_images: FxHashMap<ImageData, ImflowImageBuffer> = FxHashMap::default();
        loaded_images.reserve(available_images.len());

        let mut loaded_thumbnails: FxHashMap<_, _> = FxHashMap::default();
        loaded_thumbnails.reserve(available_images.len());
        loaded_thumbnails.extend(receiver.iter());

        let mut load_times: VecDeque<_> = VecDeque::default();
        load_times.reserve(available_images.len());

        let total_time = total_start.elapsed();
        println!(
            "all thumbnails load time: {:?} for {}",
            total_time,
            loaded_thumbnails.len()
        );

        let image = load_image(&new_path.clone());
        // let (path, image) = first_image_thread.join().unwrap();
        loaded_images.insert(new_path.clone(), image);
        let mut state = Self {
            current_image_id,
            loaded_images,
            available_images,
            current_image_path: new_path,
            pool,
            loader_rx,
            loader_tx,
            currently_loading,
            loaded_images_thumbnails: loaded_thumbnails,
            image_changed: true,
            load_times,
        };

        state.preload_next_images(PRELOAD_NEXT_IMAGE_N);

        state
    }

    pub fn set_rating(&mut self, rating: i32) {
        let path = self.current_image_path.path.clone();
        // println!("Writing {} to {:?}", rating, path);
        let mut exiftool = ExifTool::new().unwrap();
        exiftool
            .write_tag(path.as_path(), "Rating", rating, &["-overwrite_original"])
            .unwrap();
        self.current_image_path.rating = rating;
        self.available_images[self.current_image_id].rating = rating;
        if let Some(full) = self.loaded_images.get_mut(&self.current_image_path.clone()) {
            full.rating = rating;
        }
        if let Some(thumbnail) = self
            .loaded_images_thumbnails
            .get_mut(&self.current_image_path.clone())
        {
            thumbnail.rating = rating;
        }
    }

    pub fn get_current_rating(&self) -> i32 {
        self.current_image_path.rating
    }

    pub fn preload_next_images(&mut self, n: usize) {
        for image in self
            .available_images
            .clone()
            .iter()
            .skip(self.current_image_id)
            .take(n)
        {
            self.request_load(image.clone());
        }
    }

    pub fn request_load(&mut self, path: ImageData) {
        if self.loaded_images.contains_key(&path) || self.currently_loading.contains(&path) {
            return;
        }
        let tx = self.loader_tx.clone();
        self.currently_loading.insert(path.clone());

        self.pool.execute(move || {
            let image = load_image(&path.clone());
            let _ = tx.send((path, image));
        });
    }

    pub fn check_loaded_images(&mut self) {
        while let Ok((path, image)) = self.loader_rx.try_recv() {
            self.loaded_images.insert(path.clone(), image);
            self.currently_loading.remove(&path);
            self.load_times.push_front(path);

            if self.loaded_images.len() > MAX_LOADED_IMAGES {
                self.evict_images(15);
            }
        }
    }

    pub fn next_image(&mut self, change: i32, filter: Option<FileFilters>) {
        let mut next_id = self.current_image_id as i32;
        loop {
            next_id += change;
            if next_id < 0 || next_id > self.available_images.len() as i32 - 1 {
                break;
            }
            if let Some(filter) = &filter {
                // println("matching on filter");
                if self.filter_image(&self.available_images[next_id as usize], filter) {
                    self.current_image_id = next_id as usize;
                    break;
                }
            } else {
                self.current_image_id = next_id as usize;
                break;
            }
        }

        let new_path = self.available_images[self.current_image_id].clone();
        if !self.loaded_images.contains_key(&new_path) {
            self.request_load(new_path.clone());
        }
        self.current_image_path = new_path;
        self.preload_next_images(PRELOAD_NEXT_IMAGE_N);
        self.image_changed = true;
    }

    pub fn select_image(&mut self, selected_image: ImageData) {
        if !self.loaded_images.contains_key(&selected_image) {
            self.request_load(selected_image.clone());
        }
        let id = self
            .available_images
            .iter()
            .position(|i| *i == selected_image)
            .unwrap();
        self.current_image_path = selected_image;
        self.current_image_id = id;
        self.preload_next_images(PRELOAD_NEXT_IMAGE_N);
        self.image_changed = true;
    }

    pub fn get_current_image(&self) -> Option<&ImflowImageBuffer> {
        self.loaded_images.get(&self.current_image_path)
    }

    pub fn get_image(&self, path: &ImageData) -> Option<&ImflowImageBuffer> {
        self.loaded_images.get(path)
    }

    pub fn get_thumbnail_id(&self, id: usize) -> &ImflowImageBuffer {
        let path = self.available_images.get(id).unwrap();
        self.loaded_images_thumbnails.get(path).unwrap()
    }

    pub fn get_thumbnail_hash(&self, hash: String) -> &ImflowImageBuffer {
        self.loaded_images_thumbnails
            .iter()
            .find(|f| f.0.get_hash_str() == hash)
            .unwrap()
            .1
    }

    pub fn get_thumbnail(&mut self) -> &ImflowImageBuffer {
        if self
            .loaded_images_thumbnails
            .contains_key(&self.current_image_path)
        {
            return self
                .loaded_images_thumbnails
                .get(&self.current_image_path)
                .unwrap();
        }

        let buf = load_thumbnail(&self.current_image_path);
        self.loaded_images_thumbnails
            .insert(self.current_image_path.clone(), buf);
        return self
            .loaded_images_thumbnails
            .get(&self.current_image_path)
            .unwrap();
    }

    pub fn get_filtered_images(&self, filter: &FileFilters) -> Vec<ImageData> {
        self.available_images
            .iter()
            .filter(|f| filter.rating[f.rating.clamp(0, 5) as usize])
            .filter(|f| filter.file_format[&f.format])
            .filter(|f| {
                f.path
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .contains(&filter.name)
            })
            .map(|f| f.clone())
            .collect::<Vec<ImageData>>()
    }

    fn filter_image(&self, image: &ImageData, filter: &FileFilters) -> bool {
        filter.rating[image.rating.clamp(0, 5) as usize]
            && filter.file_format[&image.format]
            && image
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains(&filter.name)
    }

    fn evict_images(&mut self, count: usize) {
        for _ in 0..count {
            if let Some(loaded_image) = self.load_times.pop_back() {
                println!("Cache eviction: {:?}", loaded_image.path);
                let _ = self.loaded_images.remove(&loaded_image);
            }
        }
    }
}
