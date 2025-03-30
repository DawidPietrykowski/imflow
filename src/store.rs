use crate::image::{ImageData, load_thumbnail};
use crate::image::{ImflowImageBuffer, load_available_images, load_image};
use rexiv2::Metadata;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;
use threadpool::ThreadPool;

const PRELOAD_NEXT_IMAGE_N: usize = 16;

pub struct ImageStore {
    pub(crate) current_image_id: usize,
    pub(crate) loaded_images: HashMap<ImageData, ImflowImageBuffer>,
    pub(crate) loaded_images_thumbnails: HashMap<ImageData, ImflowImageBuffer>,
    pub(crate) available_images: Vec<ImageData>,
    pub current_image_path: ImageData,
    pub(crate) pool: ThreadPool,
    pub(crate) loader_rx: mpsc::Receiver<(ImageData, ImflowImageBuffer)>,
    pub(crate) loader_tx: mpsc::Sender<(ImageData, ImflowImageBuffer)>,
    pub(crate) currently_loading: HashSet<ImageData>,
}

impl ImageStore {
    pub fn new(path: PathBuf) -> Self {
        let current_image_id: usize = 0;
        let mut loaded_images: HashMap<ImageData, ImflowImageBuffer> = HashMap::new();
        let mut loaded_thumbnails: HashMap<ImageData, ImflowImageBuffer> = HashMap::new();
        let available_images = load_available_images(path);
        let new_path = available_images[0].clone();

        let (loader_tx, loader_rx) = mpsc::channel();

        let pool = ThreadPool::new(32);

        let currently_loading = HashSet::new();

        let total_start = Instant::now();
        let mut loaded = 0;
        let to_load = available_images.len();
        for path in &available_images {
            let buf = load_thumbnail(path);
            loaded_thumbnails.insert(path.clone(), buf);
            loaded += 1;
            println!("{}/{}", loaded, to_load);
        }
        let total_time = total_start.elapsed();
        println!(
            "all thumbnails load time: {:?} for {}",
            total_time,
            loaded_thumbnails.len()
        );

        let path = available_images[0].clone();
        let image = load_image(&path.clone());
        loaded_images.insert(path, image);
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
        };

        state.preload_next_images(PRELOAD_NEXT_IMAGE_N);

        state
    }

    pub fn set_rating(&mut self, rating: i32) {
        let meta = Metadata::new_from_path(self.current_image_path.path.clone());
        match meta {
            Ok(meta) => {
                meta.set_tag_numeric("Xmp.xmp.Rating", rating).unwrap();
                meta.save_to_file(self.current_image_path.path.clone())
                    .unwrap();
            }
            Err(e) => panic!("{:?}", e),
        }
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
        let imbuf = if let Some(full) = self.get_current_image() {
            // println!("full");
            full
        } else {
            // TODO: this assumes loaded thumbnail
            self.loaded_images_thumbnails
                .get(&self.current_image_path)
                .unwrap()
        };
        imbuf.rating
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
        }
    }

    pub fn next_image(&mut self, change: i32) {
        self.current_image_id = (self.current_image_id as i32 + change)
            .clamp(0, self.available_images.len() as i32 - 1)
            as usize;

        let new_path = self.available_images[self.current_image_id].clone();
        if !self.loaded_images.contains_key(&new_path) {
            self.request_load(new_path.clone());
        }
        self.current_image_path = new_path;
        self.preload_next_images(PRELOAD_NEXT_IMAGE_N);
    }

    pub fn get_current_image(&self) -> Option<&ImflowImageBuffer> {
        self.loaded_images.get(&self.current_image_path)
    }

    pub fn get_image(&self, path: &ImageData) -> Option<&ImflowImageBuffer> {
        self.loaded_images.get(path)
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
}
