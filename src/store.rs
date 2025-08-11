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

pub const EDIT_TAG: &str = "edit";
pub const CROP_TAG: &str = "crop";

#[derive(PartialEq)]
pub enum TagAction {
    Add,
    Remove,
    Toggle,
}

#[derive(Clone)]
pub struct FileFilters {
    pub rating: [bool; 6],
    pub name: String,
    pub file_format: HashMap<ImageFormat, bool>,
    pub tags: HashMap<String, bool>,
}

impl Default for FileFilters {
    fn default() -> Self {
        let mut formats = HashMap::new();
        formats.insert(ImageFormat::Jpg, true);
        formats.insert(ImageFormat::Jxl, true);
        formats.insert(ImageFormat::Heif, true);
        // TODO: Refactor such that it's not possible to miss an enum
        formats.insert(ImageFormat::Video, true);
        let mut tags = HashMap::new();
        tags.insert(EDIT_TAG.to_string(), false);
        tags.insert(CROP_TAG.to_string(), false);
        FileFilters {
            rating: [true; 6],
            name: "".to_string(),
            file_format: formats,
            tags,
        }
    }
}

impl FileFilters {
    fn filter_image(&self, image: &ImageData) -> bool {
        self.rating[image.rating.clamp(0, 5) as usize]
            && self.file_format[&image.format]
            && image
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains(&self.name)
            && (!self.tags.iter().any(|f| *f.1)
                || self
                    .tags
                    .iter()
                    .filter(|f| *f.1)
                    .all(|tag| image.tags.contains(tag.0)))
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
    previous_id: Option<usize>,
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
            previous_id: None,
        };

        state.preload_next_images(PRELOAD_NEXT_IMAGE_N, None);

        state
    }

    pub fn set_rating(&mut self, rating: i32) {
        let current_image = &mut self.available_images[self.current_image_id];
        let path = current_image.path.clone();

        // println!("Writing {} to {:?}", rating, path);
        let mut exiftool = ExifTool::new().unwrap();
        exiftool
            .write_tag(path.as_path(), "Rating", rating, &["-overwrite_original"])
            .unwrap();
        self.current_image_path.rating = rating;
        current_image.rating = rating;
    }

    pub fn set_tag(&mut self, tag: String, action: TagAction) {
        let current_image = &mut self.available_images[self.current_image_id];
        let contains = current_image.tags.contains(&tag);
        let add = match action {
            TagAction::Add => true,
            TagAction::Remove => false,
            TagAction::Toggle => !contains,
        };

        if add && contains {
            return;
        } else if !add && !contains {
            return;
        }

        let path = current_image.path.clone();
        let mut exiftool = ExifTool::new().unwrap();
        let action_char = match add {
            true => '+',
            false => '-',
        };
        let tag_arg = format!("-{}{}={}", "XMP:TagsList", action_char, tag);

        let path_str = path.to_string_lossy();
        let mut args = vec![tag_arg.as_str()];
        args.extend_from_slice(&["-overwrite_original"]);
        args.push(path_str.as_ref());

        exiftool.execute_raw(&args).unwrap();

        match add {
            true => {
                current_image.tags.push(tag.clone());
                self.current_image_path.tags.push(tag.clone());
            }
            false => {
                let pos = current_image.tags.iter().position(|t| *t == tag).unwrap();
                current_image.tags.remove(pos);

                let pos = self
                    .current_image_path
                    .tags
                    .iter()
                    .position(|t| *t == tag)
                    .unwrap();
                self.current_image_path.tags.remove(pos);
            }
        }
    }

    pub fn get_current_rating(&self) -> i32 {
        self.current_image_path.rating
    }

    pub fn preload_next_images(&mut self, n: usize, filter: Option<&FileFilters>) {
        for i in 1..=n {
            if let Some(next_id) = self.get_next_image_id(i as i32, filter) {
                self.request_load(self.available_images[next_id].clone());
            } else {
                break;
            }
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

    fn get_next_image_id(&mut self, change: i32, filter: Option<&FileFilters>) -> Option<usize> {
        let mut next_id = self.current_image_id as i32;
        loop {
            next_id += change;
            if next_id < 0 || next_id > self.available_images.len() as i32 - 1 {
                // restore to original id
                return None;
            }
            if let Some(filter) = &filter {
                // println("matching on filter");
                if filter.filter_image(&self.available_images[next_id as usize]) {
                    break;
                }
            } else {
                break;
            }
        }

        Some(next_id as usize)
    }

    pub fn next_image(&mut self, change: i32, filter: Option<&FileFilters>) {
        if let Some(next_id) = self.get_next_image_id(change, filter) {
            self.set_image(next_id, filter);
        }
    }

    pub fn select_image(&mut self, selected_image: ImageData, filter: Option<&FileFilters>) {
        let id = self
            .available_images
            .iter()
            .position(|i| *i == selected_image)
            .unwrap();

        self.set_image(id, filter);
    }

    fn set_image(&mut self, next_id: usize, filter: Option<&FileFilters>) {
        self.previous_id = Some(self.current_image_id);

        let new_image = self.available_images[next_id as usize].clone();
        if !self.loaded_images.contains_key(&new_image) {
            self.request_load(new_image.clone());
        }
        self.current_image_path = new_image;
        self.current_image_id = next_id as usize;
        self.preload_next_images(PRELOAD_NEXT_IMAGE_N, filter);
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
            .filter(|f| filter.filter_image(f))
            .map(|f| f.clone())
            .collect::<Vec<ImageData>>()
    }

    fn evict_images(&mut self, count: usize) {
        for _ in 0..count {
            if let Some(loaded_image) = self.load_times.pop_back() {
                println!("Cache eviction: {:?}", loaded_image.path);
                let _ = self.loaded_images.remove(&loaded_image);
            }
        }
    }

    pub fn last_image(&mut self, filter: Option<&FileFilters>) {
        if let Some(previous_id) = self.previous_id {
            self.set_image(previous_id, filter);
        }
    }
}
