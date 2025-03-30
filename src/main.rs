//! This example showcases an interactive version of the Game of Life, invented
//! by John Conway. It leverages a `Canvas` together with other widgets.
// use grid::Grid;

use std::collections::HashMap;
use std::fs::{self};
use std::io::Read;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{self, Duration, Instant};

use iced::futures::AsyncReadExt;
use iced::widget::shader::wgpu::core::command::LoadOp;
// use iced::time::milliseconds;
use iced::widget::image::FilterMethod;
use iced::widget::{
    Column, Container, button, center, checkbox, column, container, pick_list, row, slider, text,
};
use iced::{Center, Element, Fill, Length, Size, Subscription, Task, Theme, keyboard};
use image::{self, DynamicImage, EncodableLayout, GenericImageView, ImageBuffer, ImageReader};

use imflow::image::{
    flatten_image_image, flatten_zune_image, load_available_images, load_image_argb, load_image_argb_imagers, load_thumbnail, map_file, map_file_path, read_zune_image, Approach, ImflowImageBuffer
};
use minifb::{Key, Window, WindowOptions};
use zune_image::codecs::qoi::zune_core::options::DecoderOptions; // for general image operations
// use image::io::Reader as ImageReader; // specifically for Reader

use std::sync::Arc;

// use winit::{
//     application::ApplicationHandler,
//     event::WindowEvent,
//     event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
//     window::{Window, WindowId},
// };
// struct State {
//     window: Arc<Window>,
//     device: wgpu::Device,
//     queue: wgpu::Queue,
//     size: winit::dpi::PhysicalSize<u32>,
//     surface: wgpu::Surface<'static>,
//     surface_format: wgpu::TextureFormat,
// }

// impl State {
//     async fn new(window: Arc<Window>) -> State {
//         let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
//         let adapter = instance
//             .request_adapter(&wgpu::RequestAdapterOptions::default())
//             .await
//             .unwrap();
//         let (device, queue) = adapter
//             .request_device(&wgpu::DeviceDescriptor::default(), None)
//             .await
//             .unwrap();

//         let size = window.inner_size();

//         let surface = instance.create_surface(window.clone()).unwrap();
//         let cap = surface.get_capabilities(&adapter);
//         let surface_format = cap.formats[0];

//         let state = State {
//             window,
//             device,
//             queue,
//             size,
//             surface,
//             surface_format,
//         };

//         // Configure surface for the first time
//         state.configure_surface();

//         state
//     }

//     fn get_window(&self) -> &Window {
//         &self.window
//     }

//     fn configure_surface(&self) {
//         let surface_config = wgpu::SurfaceConfiguration {
//             usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
//             format: self.surface_format,
//             // Request compatibility with the sRGB-format texture view we‘re going to create later.
//             view_formats: vec![self.surface_format.add_srgb_suffix()],
//             alpha_mode: wgpu::CompositeAlphaMode::Auto,
//             width: self.size.width,
//             height: self.size.height,
//             desired_maximum_frame_latency: 2,
//             present_mode: wgpu::PresentMode::AutoVsync,
//         };
//         self.surface.configure(&self.device, &surface_config);
//     }

//     fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
//         self.size = new_size;

//         // reconfigure the surface
//         self.configure_surface();
//     }

//     // fn render(&mut self) {
//     //     // Create texture view
//     //     let surface_texture = self
//     //         .surface
//     //         .get_current_texture()
//     //         .expect("failed to acquire next swapchain texture");
//     //     let texture_view = surface_texture
//     //         .texture
//     //         .create_view(&wgpu::TextureViewDescriptor {
//     //             // Without add_srgb_suffix() the image we will be working with
//     //             // might not be "gamma correct".
//     //             format: Some(self.surface_format.add_srgb_suffix()),
//     //             ..Default::default()
//     //         });

//     //     // Renders a GREEN screen
//     //     let mut encoder = self.device.create_command_encoder(&Default::default());
//     //     // Create the renderpass which will clear the screen.
//     //     let renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
//     //         label: None,
//     //         color_attachments: &[Some(wgpu::RenderPassColorAttachment {
//     //             view: &texture_view,
//     //             resolve_target: None,
//     //             ops: wgpu::Operations {
//     //                 load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
//     //                 store: wgpu::StoreOp::Store,
//     //             },
//     //         })],
//     //         depth_stencil_attachment: None,
//     //         timestamp_writes: None,
//     //         occlusion_query_set: None,
//     //     });

//     //     // If you wanted to call any drawing commands, they would go here.

//     //     // End the renderpass.
//     //     drop(renderpass);

//     //     // Submit the command in the queue to execute
//     //     self.queue.submit([encoder.finish()]);
//     //     self.window.pre_present_notify();
//     //     surface_texture.present();
//     // }

//     fn render(&mut self) {
//             let mmap = map_file("test_images/20240811-194516_DSC02274.JPG").unwrap();
//             println!("mapped file");
//             let img = read_zune_image(mmap.deref()).unwrap();
//             let width = img.dimensions().0 as u32;
//             let height = img.dimensions().1 as u32;
//             println!("loaded");
//             let flat = flatten_zune_image(&img);
//             println!("flattened");

//         let rgb_bytes = flat[0].as_slice();
//     // Assuming `self.rgb_bytes` is your buffer containing RGB data.
//     let texture_extent = wgpu::Extent3d {
//         width: width,
//         height: height,
//         depth_or_array_layers: 1,
//     };

//     // Create a wgpu texture
//     let texture = self.device.create_texture(&wgpu::TextureDescriptor {
//         label: Some("RGB Texture"),
//         size: texture_extent,
//         mip_level_count: 1,
//         sample_count: 1,
//         dimension: wgpu::TextureDimension::D2,
//         format: wgpu::TextureFormat::Rgba8Unorm, // It's better to use RGBA with proper padding
//         usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
//         view_formats: &[],
//     });

//     // Upload your RGB data into the texture
//     self.queue.write_texture(
//         wgpu::TexelCopyTextureInfo{
//             texture: &texture,
//             mip_level: 0,
//             origin: wgpu::Origin3d::ZERO,
//             aspect: wgpu::TextureAspect::All,
//         },
//         &rgb_bytes,
//         wgpu::TexelCopyBufferLayout {
//             offset: 0,
//             bytes_per_row: Some(4 * width),  // Assuming padded row length
//             rows_per_image: Some(height),
//         },
//         texture_extent,
//     );

//     // Create a texture view
//     let surface_texture = self
//         .surface
//         .get_current_texture()
//         .expect("failed to acquire next swapchain texture");

//     let texture_view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor {
//         format: Some(self.surface_format.add_srgb_suffix()),
//         ..Default::default()
//     });

//     let rgb_texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

//     let mut encoder = self.device.create_command_encoder(&Default::default());

//     // Create the renderpass
//     let mut renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
//         label: None,
//         color_attachments: &[Some(wgpu::RenderPassColorAttachment {
//             view: &texture_view,
//             resolve_target: None,
//             ops: wgpu::Operations {
//                 load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
//                 store: wgpu::StoreOp::Store,
//             },
//         })],
//         depth_stencil_attachment: None,
//         timestamp_writes: None,
//         occlusion_query_set: None,
//     });

//     // Bind and draw
//     // renderpass.set_pipeline(&self.pipeline);  // Ensure self.pipeline is your render pipeline setup
//     // renderpass.set_bind_group(0, &self.texture_bind_group, &[]); // Assuming you have a bind group which holds the texture
//     renderpass.draw(0..3, 0..1); // Draws a triangle to cover the viewport, modify as needed for quads

//     // End the renderpass
//     drop(renderpass);

//     // Submit the command buffer
//     // self.queue.submit(iter::once(encoder.finish()));
//     self.window.pre_present_notify();
//     surface_texture.present();
// }
// }

// #[derive(Default)]
// struct App {
//     state: Option<State>,
// }

// impl ApplicationHandler for App {
//     fn resumed(&mut self, event_loop: &ActiveEventLoop) {
//         // Create window object
//         let window = Arc::new(
//             event_loop
//                 .create_window(Window::default_attributes())
//                 .unwrap(),
//         );

//         let state = pollster::block_on(State::new(window.clone()));
//         self.state = Some(state);

//         window.request_redraw();
//     }

//     fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
//         let state = self.state.as_mut().unwrap();
//         match event {
//             WindowEvent::CloseRequested => {
//                 println!("The close button was pressed; stopping");
//                 event_loop.exit();
//             }
//             WindowEvent::RedrawRequested => {
//                 state.render();
//                 // Emits a new redraw requested event.
//                 state.get_window().request_redraw();
//             }
//             WindowEvent::Resized(size) => {
//                 // Reconfigures the size of the surface. We do not re-render
//                 // here as this event is always followed up by redraw request.
//                 state.resize(size);
//             }
//             _ => (),
//         }
//     }
// }
// pub fn main() -> iced::Result {
//     tracing_subscriber::fmt::init();

//     iced::application("Game of Life - Iced", GameOfLife::update, GameOfLife::view)
//         .subscription(GameOfLife::subscription)
//         .theme(|_| Theme::Dark)
//         .antialiasing(true)
//         .centered()
//         .window_size(Size::new(1500.0, 1000.0))
//         .run()
// }

// fn main() {
//     let mut window = match Window::new("Test", 640, 400, WindowOptions::default()) {
//    Ok(win) => win,
//    Err(err) => {
//        println!("Unable to create window {}", err);
//        return;
//    }
//    }
// }

// fn main() {
//     // wgpu uses `log` for all of our logging, so we initialize a logger with the `env_logger` crate.
//     //
//     // To change the log level, set the `RUST_LOG` environment variable. See the `env_logger`
//     // documentation for more information.
//     env_logger::init();

//     let event_loop = EventLoop::new().unwrap();

//     // When the current loop iteration finishes, immediately begin a new
//     // iteration regardless of whether or not new events are available to
//     // process. Preferred for applications that want to render as fast as
//     // possible, like games.
//     event_loop.set_control_flow(ControlFlow::Poll);

//     // When the current loop iteration finishes, suspend the thread until
//     // another event arrives. Helps keeping CPU utilization low if nothing
//     // is happening, which is preferred if the application might be idling in
//     // the background.
//     // event_loop.set_control_flow(ControlFlow::Wait);

//     let mut app = App::default();
//     event_loop.run_app(&mut app).unwrap();
// }
//

struct State {
    current_image_id: usize,
    loaded_images: HashMap<PathBuf, ImflowImageBuffer>,
    available_images: Vec<PathBuf>,
    current_image_path: PathBuf,
}

impl State {
    fn new() -> Self {
        let current_image_id: usize = 0;
        let mut loaded_images: HashMap<PathBuf, ImflowImageBuffer> = HashMap::new();
        let available_images = load_available_images("./test_images".into());
        let new_path = available_images[0].clone();
        let current_image = load_image_argb_imagers(new_path.clone());
        loaded_images.insert(new_path.clone(), current_image);
        Self {
            current_image_id,
            loaded_images,
            available_images,
            current_image_path: new_path,
        }
    }

    fn next_image(&mut self, change: i32) {
        self.current_image_id = (self.current_image_id as i32 + change)
            .clamp(0, self.available_images.len() as i32 - 1)
            as usize;
        let new_path = self.available_images[self.current_image_id].clone();
        if !self.loaded_images.contains_key(&new_path) {
            let new_image = load_image_argb(new_path.clone());
            self.loaded_images.insert(new_path.clone(), new_image);
        }
        self.current_image_path = new_path;
    }

    fn get_current_image(&self) -> &ImflowImageBuffer {
        self.loaded_images.get(&self.current_image_path).unwrap()
    }
}

fn main() {
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    let mut window = Window::new(
        "Test - ESC to exit",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    )
    .unwrap_or_else(|e| {
        panic!("{}", e);
    });

    window.set_target_fps(60);

    let mut state = State::new();
    show_image(&mut window, state.get_current_image());

    while window.is_open() && !window.is_key_down(Key::Escape) {
        window.update();
        if window.is_key_down(Key::Right) {
            state.next_image(1);
            show_image(&mut window, state.get_current_image());
        } else if window.is_key_down(Key::Left) {
            state.next_image(-1);
            show_image(&mut window, state.get_current_image());
        }
    }
}

fn show_image(window: &mut Window, image: &ImflowImageBuffer) {
    window
        .update_with_buffer(&image.argb_buffer, image.width, image.height)
        .unwrap();
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
                        load_thumbnail(path.to_str().unwrap(), Approach::ImageRs).unwrap(),
                    );
                }
            }
        }

        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::on_key_press(|key, _modifiers| match key {
            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some(Message::Next(1)),
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some(Message::Next(-1)),
            _ => None,
        })
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
