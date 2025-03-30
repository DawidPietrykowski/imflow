//! This example showcases an interactive version of the Game of Life, invented
//! by John Conway. It leverages a `Canvas` together with other widgets.
// use grid::Grid;

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{self, Duration};

use iced::futures::AsyncReadExt;
use iced::widget::shader::wgpu::core::command::LoadOp;
// use iced::time::milliseconds;
use iced::widget::image::FilterMethod;
use iced::widget::{
    Column, Container, button, center, checkbox, column, container, pick_list, row, slider, text,
};
use iced::{Center, Element, Fill, Length, Size, Subscription, Task, Theme};
use image::{self, DynamicImage, EncodableLayout, ImageBuffer, ImageReader};
use itertools::Itertools;
use zune_image::codecs::qoi::zune_core::options::DecoderOptions; // for general image operations
// use image::io::Reader as ImageReader; // specifically for Reader

pub fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    iced::application("Game of Life - Iced", GameOfLife::update, GameOfLife::view)
        .subscription(GameOfLife::subscription)
        .theme(|_| Theme::Dark)
        .antialiasing(true)
        .centered()
        .window_size(Size::new(1500.0, 1000.0))
        .run()
}

struct GameOfLife {
    is_playing: bool,
    queued_ticks: usize,
    speed: usize,
    next_speed: Option<usize>,
    version: usize,
    image_filter_method: FilterMethod,
    current_image: Option<PathBuf>,
    width: u32,
    available_images: Vec<PathBuf>,
    current_image_id: usize,
    loaded_images: HashMap<PathBuf, iced::widget::image::Handle>,
}

#[derive(Debug, Clone)]
enum Message {
    TogglePlayback,
    ToggleGrid(bool),
    Clear,
    SpeedChanged(f32),
    Tick,
    Next(i32),
    ImageWidthChanged(u32),
    ImageUseNearestToggled(bool),
}

impl GameOfLife {
    fn new() -> Self {
        let mut dir: Vec<PathBuf> = fs::read_dir(Path::new("./test_images"))
            .unwrap()
            .map(|f| f.unwrap().path())
            .collect();
        dir.sort();
        let mut res = Self {
            is_playing: false,
            queued_ticks: 0,
            speed: 5,
            next_speed: None,
            version: 0,
            image_filter_method: FilterMethod::Nearest,
            width: 1400,
            current_image: Some(dir.first().unwrap().clone()),
            available_images: dir,
            current_image_id: 0,
            loaded_images: HashMap::new(),
        };
        let _ = res.update(Message::Next(0));
        res
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                self.queued_ticks = (self.queued_ticks + 1).min(self.speed);

                // if let Some(task) = self.grid.tick(self.queued_ticks) {
                //     if let Some(speed) = self.next_speed.take() {
                //         self.speed = speed;
                //     }

                //     self.queued_ticks = 0;

                //     let version = self.version;

                //     // return Task::perform(task, Message::Grid.with(version));
                // }
            }
            Message::TogglePlayback => {
                self.is_playing = !self.is_playing;
            }
            Message::ToggleGrid(show_grid_lines) => {
                // self.grid.toggle_lines(show_grid_lines);
            }
            Message::Clear => {
                // self.grid.clear();
                self.version += 1;
            }
            Message::SpeedChanged(speed) => {
                if self.is_playing {
                    self.next_speed = Some(speed.round() as usize);
                } else {
                    self.speed = speed.round() as usize;
                }
            }
            Message::ImageWidthChanged(image_width) => {
                self.width = image_width;
            }
            Message::ImageUseNearestToggled(use_nearest) => {
                self.image_filter_method = if use_nearest {
                    FilterMethod::Nearest
                } else {
                    FilterMethod::Linear
                };
            }
            Message::Next(change) => {
                let elements = self.available_images.len() as i32;
                let new_id = (self.current_image_id as i32 + change).clamp(0, elements - 1);
                println!(
                    "updated id: {} from {} total {}",
                    new_id, self.current_image_id, elements
                );
                self.current_image_id = new_id as usize;
                let path = self
                    .available_images
                    .get(self.current_image_id)
                    .unwrap()
                    .clone();
                self.current_image = Some(path.clone());
                if !self.loaded_images.contains_key(&path.to_path_buf()) {
                    self.loaded_images.insert(
                        path.to_path_buf(),
                        load_thumbnail(path.to_str().unwrap()).unwrap(),
                    );
                }
            }
        }

        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.is_playing {
            iced::time::every(Duration::from_millis(1000 / self.speed as u64))
                .map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let version = self.version;
        let selected_speed = self.next_speed.unwrap_or(self.speed);
        let controls = view_controls(
            self.is_playing,
            true,
            // self.grid.are_lines_visible(),
            selected_speed,
            // self.grid.preset(),
        );

        let content = column![
            // image("/media/nfs/sphotos/Images/24-08-11-Copenhagen/24-08-12/20240812-175614_DSC03844.JPG").into(),
            // self.grid.view().map(Message::Grid.with(version)),
            self.image(),
            controls,
        ]
        .height(Fill);

        container(content).width(Fill).height(Fill).into()
        // image("/media/nfs/sphotos/Images/24-08-11-Copenhagen/24-08-12/20240812-175614_DSC03844.JPG").into()
    }

    fn image(&self) -> Column<Message> {
        let width = self.width;
        let filter_method = self.image_filter_method;

        Self::container("Image")
            .push("An image that tries to keep its aspect ratio.")
            .push(self.ferris(
                width,
                filter_method,
                self.current_image.as_ref().unwrap().as_ref(),
            ))
            .push(slider(100..=1500, width, Message::ImageWidthChanged))
            .push(text!("Width: {width} px").width(Fill).align_x(Center))
            .push(
                checkbox(
                    "Use nearest interpolation",
                    filter_method == FilterMethod::Nearest,
                )
                .on_toggle(Message::ImageUseNearestToggled),
            )
            .align_x(Center)
    }

    fn container(title: &str) -> Column<'_, Message> {
        column![text(title).size(50)].spacing(20)
    }

    fn ferris<'a>(
        &self,
        width: u32,
        filter_method: iced::widget::image::FilterMethod,
        path: &Path,
    ) -> Container<'a, Message> {
        if self.loaded_images.get(path).is_none() {
            return center(text("loading"));
        }
        let img = iced::widget::image::Image::new(self.loaded_images.get(path).unwrap());
        center(
            // This should go away once we unify resource loading on native
            // platforms
            img.filter_method(filter_method)
                .width(Length::Fixed(width as f32)),
        )
    }
}

impl Default for GameOfLife {
    fn default() -> Self {
        Self::new()
    }
}

fn view_controls<'a>(
    is_playing: bool,
    is_grid_enabled: bool,
    speed: usize,
    // preset: Preset,
) -> Element<'a, Message> {
    let playback_controls = row![
        button(if is_playing { "Pause" } else { "Play" }).on_press(Message::TogglePlayback),
        button("Previous")
            .on_press(Message::Next(-1))
            .style(button::secondary),
        button("Next")
            .on_press(Message::Next(1))
            .style(button::secondary),
    ]
    .spacing(10);

    let speed_controls = row![
        slider(1.0..=1000.0, speed as f32, Message::SpeedChanged),
        text!("x{speed}").size(16),
    ]
    .align_y(Center)
    .spacing(10);

    row![
        playback_controls,
        speed_controls,
        // checkbox("Grid", is_grid_enabled).on_toggle(Message::ToggleGrid),
        // row![
        //     pick_list(preset::ALL, Some(preset), Message::PresetPicked),
        //     button("Clear")
        //         .on_press(Message::Clear)
        //         .style(button::danger)
        // ]
        // .spacing(10)
    ]
    .padding(10)
    .spacing(20)
    .align_y(Center)
    .into()
}

fn load_thumbnail(path: &str) -> Result<iced::widget::image::Handle, String> {
    // let file = File::open(path).map_err(|e| e.to_string())?;
    // let mmap = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| e.to_string())?;
    // println!("mapped file");
    // let img = zune_image::image::Image::read(&*mmap, DecoderOptions::default()).unwrap();
    let img = zune_image::image::Image::open_with_options(path, DecoderOptions::default()).unwrap();
    // let img = image::load_from_memory(&mmap)
    // .map_err(|e| e.to_string())?
    // .decode()
    // .map_err(|e| e.to_string())?;

    // let thumbnail = img.thumbnail(128, 128);
    let width = img.dimensions().0 as u32;
    let height = img.dimensions().1 as u32;
    println!("loaded");
    let flat = img.flatten_to_u8();
    println!("flattened");
    let rgba = convert_rgb_to_rgba(flat);
    println!("rgbad");
    // let leaked_slice = Box::leak(rgba[0].clone().into_boxed_slice());
    // let slice = rgba[0].as_slice().clone();
    let conv = iced::widget::image::Handle::from_rgba(
        width, height, // leaked_slice.as_bytes(),
        rgba,
    );
    println!("iced");

    Ok((conv))
}

fn convert_rgb_to_rgba(rgb_data: Vec<Vec<u8>>) -> Vec<u8> {
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
