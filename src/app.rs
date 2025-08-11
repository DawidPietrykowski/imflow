use crate::egui_tools::EguiRenderer;
use egui::load::{ImageLoadResult, ImageLoader};
use egui::{
    Align, Color32, ColorImage, Event, Image, ImageSource, Key, PointerButton, Pos2, Sense, TextureOptions, Vec2
};
use egui_wgpu::wgpu::{Limits, SurfaceError};
use egui_wgpu::{ScreenDescriptor, wgpu};
use image::metadata::Orientation;
use imflow::image::{ImageData, swap_wh};
use imflow::store::{CROP_TAG, EDIT_TAG, FileFilters, ImageStore, TagAction};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::exit;
use std::sync::{Arc, RwLock};
use wgpu::util::DeviceExt;
use wgpu::{PipelineCompilationOptions, SurfaceConfiguration};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::platform::x11::WindowAttributesExtX11;
use winit::window::{Window, WindowId};

pub const MAX_IMAGE_SIZE: u32 = 8192 * 2;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Transforms {
    transform: [f32; 16], // 4x4 matrix
    width: u32,
    height: u32,
    orientation: u32,
    _padding2: u32,
}

pub(crate) struct TransformData {
    pan_x: f32,
    pan_y: f32,
    zoom: f32,
    width: u32,
    height: u32,
    orientation: Orientation,
    zoom_center_x: f32,
    zoom_center_y: f32
}

#[rustfmt::skip]
fn create_transform_matrix(data: &TransformData, scale_x: f32, scale_y: f32, zoom_center_x: f32, zoom_center_y: f32) -> [f32; 16] {
    const ZOOM_MULTIPLIER: f32 = 3.0;
    let zoom = (data.zoom).powf(ZOOM_MULTIPLIER);

    // [
    //     zoom * scale_x, 0.0,            0.0, 0.0,
    //     0.0,            zoom * scale_y, 0.0, 0.0,
    //     0.0,            0.0,            1.0, 0.0,
    //     data.pan_x,     data.pan_y,     0.0, 1.0,
    // ]
        let tx = data.pan_x + zoom_center_x * (1.0 - zoom);
        let ty = data.pan_y + zoom_center_y * (1.0 - zoom);


        [
        zoom * scale_x, 0.0,            0.0, 0.0,
        0.0,            zoom * scale_y, 0.0, 0.0,
        0.0,            0.0,            1.0, 0.0,
        tx, ty, 0.0, 1.0,
    ]
}

fn create_inner_render_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Inner Render Target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn setup_texture(
    device: &wgpu::Device,
    surface_config: SurfaceConfiguration,
    width: u32,
    height: u32,
) -> (
    wgpu::Texture,
    wgpu::BindGroup,
    wgpu::RenderPipeline,
    wgpu::Buffer,
) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Image texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Texture Bind Group Layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::all(),
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    let transform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Transform Uniform Buffer"),
        size: std::mem::size_of::<Transforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Create bind group with your texture
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Texture Bind Group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: transform_buffer.as_entire_binding(),
            },
        ],
    });

    let vertex_buffer_layout = wgpu::VertexBufferLayout {
        array_stride: 5 * std::mem::size_of::<f32>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            // Position
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            // UV
            wgpu::VertexAttribute {
                offset: 3 * std::mem::size_of::<f32>() as wgpu::BufferAddress,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
        ],
    };

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Texture Shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("shader.wgsl"))),
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Texture Render Pipeline"),
        layout: Some(
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Texture Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            }),
        ),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[vertex_buffer_layout],
            compilation_options: PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    (texture, bind_group, render_pipeline, transform_buffer)
}

pub struct AppState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub scale_factor: f32,
    pub egui_renderer: EguiRenderer,
    pub store: Arc<RwLock<ImageStore>>,
    pub image_texture: wgpu::Texture,
    pub bind_group: wgpu::BindGroup,
    pub render_pipeline: wgpu::RenderPipeline,
    pub transform_buffer: wgpu::Buffer,
    pub transform_data: TransformData,
    pub filters: FileFilters,
    pub selected_image: ImageData,
    pub loaded_thumbnail: bool,
    inner_texture: wgpu::Texture,
    inner_texture_view: wgpu::TextureView,
    inner_texture_id: egui::TextureId,
    inner_size: Vec2, // inner_size: (u32, u32)
}

impl AppState {
    async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        window: &Window,
        width: u32,
        height: u32,
        path: PathBuf,
    ) -> Self {
        let power_pref = wgpu::PowerPreference::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: power_pref,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let features = wgpu::Features::empty();
        let mut limits = Limits::default();
        limits.max_texture_dimension_2d = 8192 * 2;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: features,
                    required_limits: limits,
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .expect("Failed to create device");

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let selected_format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let swapchain_format = swapchain_capabilities
            .formats
            .iter()
            .find(|d| **d == selected_format)
            .expect("failed to select proper surface texture format!");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: *swapchain_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 0,
            alpha_mode: swapchain_capabilities.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &surface_config);

        let mut egui_renderer = EguiRenderer::new(&device, surface_config.format, None, 1, window);

        let image_store = ImageStore::new(path);

        // TODO: verify
        let selected_image = image_store.current_image_path.clone();
        let store = Arc::new(RwLock::new(image_store));

        let loader = ImflowEguiLoader::new(store.clone());

        egui_renderer.context().add_image_loader(Arc::new(loader));

        let scale_factor = 1.0;

        let inner_size = Vec2::new(MAX_IMAGE_SIZE as f32, MAX_IMAGE_SIZE as f32);
        let (image_texture, bind_group, render_pipeline, transform_buffer) =
            setup_texture(&device, surface_config.clone(), MAX_IMAGE_SIZE, MAX_IMAGE_SIZE);

        let (inner_texture, inner_texture_view) = create_inner_render_target(&device, MAX_IMAGE_SIZE, MAX_IMAGE_SIZE);

        let inner_texture_id = egui_renderer.renderer.register_native_texture(
            &device,
            &inner_texture_view,
            wgpu::FilterMode::Linear,
        );

        let transform_data = TransformData {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            width: 10000,
            height: 10000,
            orientation: Orientation::NoTransforms,
            zoom_center_x: 0.5,
            zoom_center_y: 0.5,
        };

        Self {
            device,
            queue,
            surface,
            surface_config,
            egui_renderer,
            scale_factor,
            store,
            image_texture,
            bind_group,
            render_pipeline,
            transform_buffer,
            transform_data,
            filters: FileFilters::default(),
            selected_image,
            loaded_thumbnail: false,
            inner_texture,
            inner_texture_view,
            inner_texture_id,
            inner_size,
        }
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub fn recreate_texture(&mut self) {
        println!("recreating texture");
        (self.inner_texture, self.inner_texture_view) = create_inner_render_target(
            &self.device,
            self.inner_size.x as u32,
            self.inner_size.y as u32,
        );

        self.inner_texture_id = self.egui_renderer.renderer.register_native_texture(
            &self.device,
            &self.inner_texture_view,
            wgpu::FilterMode::Linear,
        );
    }
}

pub struct App {
    instance: wgpu::Instance,
    state: Option<AppState>,
    window: Option<Arc<Window>>,
    path: PathBuf,
}

impl App {
    pub fn new(path: PathBuf) -> Self {
        let instance = egui_wgpu::wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        Self {
            instance,
            state: None,
            window: None,
            path,
        }
    }

    async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);
        let initial_height = 1200;
        let initial_width = (initial_height as f32 * 1.5) as u32;

        let _ = window.request_inner_size(PhysicalSize::new(initial_width, initial_height));

        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("Failed to create surface!");

        let state = AppState::new(
            &self.instance,
            surface,
            &window,
            initial_width,
            initial_width,
            self.path.clone(),
        )
        .await;

        self.window.get_or_insert(window);
        self.state.get_or_insert(state);

        self.reset_transform();
        self.update_texture(true);
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.state.as_mut().unwrap().resize_surface(width, height);
        }
        self.pan_zoom(0.0, 0.0, 0.0, 0.5, 0.5);
    }

    pub fn update_texture(&mut self, force: bool) {
        let state = self.state.as_mut().unwrap();
        if !force {
            let mut store = state.store.write().unwrap();
            store.check_loaded_images();
            let current_image_selected = state.selected_image == store.current_image_path;
            let current_quality_loaded =
                state.loaded_thumbnail == store.get_current_image().is_none();
            if current_image_selected && current_quality_loaded {
                return;
            }
        }
        {
            let mut store = state.store.write().unwrap();
            let imbuf = if let Some(full) = store.get_current_image() {
                state.loaded_thumbnail = false;
                full
            } else {
                state.loaded_thumbnail = true;
                store.get_thumbnail()
            };
            println!("updating image: {:?} {:?}", imbuf.width, imbuf.height);
            let width = imbuf.width as u32;
            let height = imbuf.height as u32;
            let buffer_u8 = unsafe {
                std::slice::from_raw_parts(
                    imbuf.rgba_buffer.as_ptr() as *const u8,
                    imbuf.rgba_buffer.len() * 4,
                )
            };

            state.transform_data.width = width;
            state.transform_data.height = height;
            state.transform_data.orientation = imbuf.orientation;

            state.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &state.image_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &buffer_u8,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * width), // 4 bytes per ARGB pixel
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            state.selected_image = store.current_image_path.clone();
        }

        self.update_transform();
    }

    fn update_transform(&mut self) {
        let state = self.state.as_mut().unwrap();

        // TODO: Remove obviously
        if state.transform_data.width < 800 {
            state.transform_data.orientation = Orientation::NoTransforms;
        }
        let (width, height) = swap_wh(
            state.transform_data.width,
            state.transform_data.height,
            state.transform_data.orientation,
        );
        let image_aspect_ratio = (width as f32) / (height as f32);
        let window_size = state.inner_size;
        let window_aspect_ratio = window_size.x / window_size.y;
        let mut scale_x = 1.0;
        let mut scale_y = 1.0;
        if window_aspect_ratio > image_aspect_ratio {
            scale_x = image_aspect_ratio / window_aspect_ratio;
        } else {
            scale_y = window_aspect_ratio / image_aspect_ratio;
        }
        let transform = create_transform_matrix(&state.transform_data, scale_x, scale_y, state.transform_data.zoom_center_x, state.transform_data.zoom_center_y);
        state.queue.write_buffer(
            &state.transform_buffer,
            0,
            bytemuck::cast_slice(&[Transforms {
                transform,
                width: width as u32,
                height: height as u32,
                orientation: state.transform_data.orientation as u32,
                _padding2: 0,
            }]),
        );
    }

    pub fn reset_transform(&mut self) {
        let state = self.state.as_mut().unwrap();
        state.transform_data.zoom = 1.0;
        state.transform_data.pan_x = 0.0;
        state.transform_data.pan_y = 0.0;

        self.update_transform();
    }

    pub fn pan_zoom(&mut self, zoom_delta: f32, pan_x: f32, pan_y: f32, zoom_center_x: f32, zoom_center_y: f32) {
        let state = self.state.as_mut().unwrap();

        state.transform_data.zoom = (state.transform_data.zoom + zoom_delta).clamp(1.0, 20.0);
        state.transform_data.pan_x += pan_x;
        state.transform_data.pan_y += -pan_y;
        state.transform_data.zoom_center_x = zoom_center_x;
        state.transform_data.zoom_center_y = zoom_center_y;

        self.update_transform();
    }

    fn handle_redraw(&mut self) {
        if let Some(window) = self.window.as_ref() {
            if let Some(min) = window.is_minimized() {
                if min {
                    println!("Window is minimized");
                    return;
                }
            }
        }

        let state = self.state.as_mut().unwrap();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [state.surface_config.width, state.surface_config.height],
            pixels_per_point: self.window.as_ref().unwrap().scale_factor() as f32
                * state.scale_factor,
        };

        let surface_texture = state.surface.get_current_texture();

        let surface_texture = match surface_texture {
            Err(SurfaceError::Outdated) => {
                // Ignoring outdated to allow resizing and minimization
                println!("wgpu surface outdated");
                return;
            }
            Err(SurfaceError::Timeout) => {
                println!("wgpu surface timeout");
                return;
            }
            Err(_) => {
                surface_texture.expect("Failed to acquire next swap chain texture");
                return;
            }
            Ok(surface_texture) => surface_texture,
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // Clear buffer with black
        {
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &state.inner_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        {
            #[repr(C)]
            #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
            struct Vertex {
                position: [f32; 3],
                tex_coords: [f32; 2],
            }

            // Quad (two triangles)
            let vertices = [
                // Position (x, y, z),   Texture coords (u, v)
                Vertex {
                    position: [-1.0, -1.0, 0.0],
                    tex_coords: [0.0, 1.0],
                }, // bottom left
                Vertex {
                    position: [-1.0, 1.0, 0.0],
                    tex_coords: [0.0, 0.0],
                }, // top left
                Vertex {
                    position: [1.0, -1.0, 0.0],
                    tex_coords: [1.0, 1.0],
                }, // bottom right
                Vertex {
                    position: [1.0, 1.0, 0.0],
                    tex_coords: [1.0, 0.0],
                }, // top right
            ];

            let indices: [u16; 6] = [0, 1, 2, 2, 1, 3];

            let vertex_buffer =
                state
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Vertex Buffer"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    });

            let index_buffer = state
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Index Buffer"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Texture Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &state.inner_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&state.render_pipeline);
            render_pass.set_bind_group(0, &state.bind_group, &[]);

            // Bind the vertex buffer
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));

            // Draw using the index buffer
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..6, 0, 0..1);
        }
        let mut pan_delta = None;
        let mut zoom_delta = None;
        let mut cursor_position = None;
        let mut reset_transform = false;
        let mut image_size = None;

        // let mut file_filters;
        let rating;
        let path;
        // let current_id;
        // let image_count;
        let filename;
        let window;
        let filtered_images;
        let current_image;
        let changed_image;
        let rating_filter;
        let tags;
        let mut selected_image = None;
        {
            let store = state.store.read().unwrap();
            rating = store.get_current_rating();
            path = store.current_image_path.clone();
            // current_id = store.current_image_id;
            // image_count = store.available_images.len();
            current_image = store.current_image_path.clone();
            filtered_images = store.get_filtered_images(&state.filters);
            changed_image = store.image_changed.clone();
            filename = path.path.file_name().unwrap();
            window = self.window.as_ref().unwrap();
            rating_filter = state.filters.rating;
            tags = path.tags;
        }
        {
            state.egui_renderer.begin_frame(window);

            egui::Window::new("Rating")
                .collapsible(false)
                .resizable(false)
                .default_width(5.0)
                .show(state.egui_renderer.context(), |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{}", rating))
                                .size(42.0)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(format!("{}", filename.to_str().unwrap()))
                                .size(10.0)
                                .strong(),
                        );
                    });
                });

            if tags.contains(&EDIT_TAG.to_string()) {
                egui::Window::new("EDIT")
                    .collapsible(false)
                    .resizable(false)
                    .default_width(10.0)
                    .title_bar(false)
                    .show(state.egui_renderer.context(), |ui| {
                        ui.label(egui::RichText::new("EDIT").monospace().size(32.0).strong());
                    });
            }
            if tags.contains(&CROP_TAG.to_string()) {
                egui::Window::new("CROP")
                    .collapsible(false)
                    .resizable(false)
                    .default_width(10.0)
                    .title_bar(false)
                    .show(state.egui_renderer.context(), |ui| {
                        ui.label(egui::RichText::new("CROP").monospace().size(32.0).strong());
                    });
            }

            egui::TopBottomPanel::bottom("Thumbnails")
                .default_height(120.0)
                .resizable(true)
                .show(state.egui_renderer.context(), |panel_ui| {
                    egui::ScrollArea::horizontal()
                        .max_width(f32::INFINITY)
                        .show(panel_ui, |ui| {
                            ui.set_max_width(f32::INFINITY);
                            ui.horizontal_centered(|horizontal| {
                                for image in filtered_images {
                                    if image.rating >= 0
                                        && image.rating < 6
                                        && !rating_filter[image.rating as usize]
                                    {
                                        continue;
                                    }
                                    let source = ImageSource::Bytes {
                                        uri: std::borrow::Cow::Owned(image.get_hash_str()),
                                        bytes: egui::load::Bytes::Static(&[]),
                                    };

                                    let image_widget = horizontal.add(
                                        egui::Image::new(source)
                                            .shrink_to_fit()
                                            .corner_radius(10)
                                            .sense(Sense::click()),
                                    );
                                    if changed_image && current_image == image {
                                        image_widget.scroll_to_me(Some(Align::Center));
                                    }
                                    if image_widget.clicked() {
                                        println!("{}", image.get_hash_str());
                                        selected_image = Some(image);
                                    }
                                }
                            });
                        });
                });

            egui::SidePanel::right("Filters").show(state.egui_renderer.context(), |ui| {
                for (i, mut rating) in state.filters.rating.iter_mut().enumerate().rev() {
                    ui.checkbox(&mut rating, format!("{} stars", i));
                }

                ui.separator();

                ui.text_edit_singleline(&mut state.filters.name);

                ui.separator();

                for (format, mut value) in state.filters.file_format.iter_mut() {
                    ui.checkbox(&mut value, format!("{}", format));
                }

                ui.separator();

                for (tag, mut value) in state.filters.tags.iter_mut() {
                    ui.checkbox(&mut value, format!("{}", tag));
                }
            });

            egui::CentralPanel::default().show(state.egui_renderer.context(), |ui| {
                let available_size = ui.available_size();
                ui.centered_and_justified(|ui| {
                    let image_response = ui.add(
                        Image::new((
                            state.inner_texture_id,
                            Vec2::new(
                                state.inner_texture.size().width as f32,
                                state.inner_texture.size().height as f32,
                            ),
                        ))
                        .texture_options(TextureOptions::LINEAR)
                        .maintain_aspect_ratio(true)
                        .fit_to_exact_size(available_size)
                        .sense(Sense::click_and_drag()),
                    );

                    image_size = Some(available_size);

                    if image_response.dragged() {
                        pan_delta = Some(image_response.drag_delta() / image_size.unwrap());
                    }

                    if image_response.clicked_by(egui::PointerButton::Secondary) {
                        reset_transform = true;
                    }

                    if image_response.hovered() {
                        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
                        if scroll_delta.y != 0.0 {
                            zoom_delta = Some(scroll_delta.y * 0.001);
                        }
                        if let Some(latest_pos) = ui.input(|i| i.pointer.latest_pos().map(Pos2::to_vec2)) {
                            let mut relative_position = latest_pos / image_size.unwrap();
                            relative_position -= Vec2::new(0.5, 0.5);
                            relative_position *= 2.0;
                            relative_position.y *= -1.0;
                            cursor_position = Some(relative_position);
                        }
                    }
                });
            });

            if let Ok(mut store) = state.store.write() {
                store.image_changed = false;
                if let Some(selected_image) = selected_image {
                    store.select_image(selected_image, Some(&state.filters));
                }
            }

            state.egui_renderer.end_frame_and_draw(
                &state.device,
                &state.queue,
                &mut encoder,
                window,
                &surface_view,
                screen_descriptor,
            );
        }

        state.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        if let Some(image_size) = image_size {
            if image_size != state.inner_size && image_size.min_elem() >= 10.0 {
                state.inner_size = image_size;
                state.recreate_texture();
                self.reset_transform();
            }
        }
        match (pan_delta, zoom_delta) {
            (None, None) => {}
            (None, Some(zoom_delta)) => self.pan_zoom(zoom_delta, 0.0, 0.0, cursor_position.unwrap().x, cursor_position.unwrap().y),
            (Some(pan_delta), None) => self.pan_zoom(0.0, pan_delta.x, pan_delta.y, cursor_position.unwrap().x, cursor_position.unwrap().y),
            (Some(pan_delta), Some(zoom_delta)) => {
                self.pan_zoom(zoom_delta, pan_delta.x, pan_delta.y, cursor_position.unwrap().x, cursor_position.unwrap().y)
            }
        }
        if reset_transform {
            self.reset_transform();
        }

        self.update_texture(false);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_base_size(LogicalSize::new(2000, 4000))
            .with_resizable(true);
        let window = event_loop.create_window(attributes).unwrap();
        pollster::block_on(self.set_window(window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        // let egui render to process the event first
        self.state
            .as_mut()
            .unwrap()
            .egui_renderer
            .handle_input(self.window.as_ref().unwrap(), &event);

        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
                let (events, _keys_down, pointer, scroll) = self
                    .state
                    .as_ref()
                    .unwrap()
                    .egui_renderer
                    .context()
                    .input(|i| {
                        (
                            i.events.clone(),
                            i.keys_down.clone(),
                            i.pointer.clone(),
                            i.smooth_scroll_delta.clone(),
                        )
                    });

                let mut updated_image = false;
                let mut reset_transform = false;
                {
                    let state = self.state.as_mut().unwrap();
                    let filters = state.filters.clone();
                    let mut store = state.store.write().unwrap();
                    events.iter().for_each(|e| {
                        if let Event::Key { key, pressed, .. } = e {
                            if !*pressed {
                                return;
                            }
                            match *key {
                                Key::ArrowLeft => {
                                    store.next_image(-1, Some(&filters));
                                    updated_image = true;
                                }
                                Key::ArrowRight => {
                                    store.next_image(1, Some(&filters));
                                    updated_image = true;
                                }
                                Key::Backslash => {
                                    store.last_image(Some(&filters));
                                    updated_image = true;
                                }
                                Key::ArrowUp => {
                                    let rating = store.get_current_rating();
                                    store.set_rating(rating + 1);
                                }
                                Key::ArrowDown => {
                                    let rating = store.get_current_rating();
                                    store.set_rating(rating - 1);
                                }
                                Key::E => {
                                    store.set_tag(EDIT_TAG.to_string(), TagAction::Toggle);
                                }
                                Key::C => {
                                    store.set_tag(CROP_TAG.to_string(), TagAction::Toggle);
                                }
                                Key::Backtick => store.set_rating(0),
                                Key::Num0 => store.set_rating(0),
                                Key::Num1 => store.set_rating(1),
                                Key::Num2 => store.set_rating(2),
                                Key::Num3 => store.set_rating(3),
                                Key::Num4 => store.set_rating(4),
                                Key::Num5 => store.set_rating(5),
                                Key::Escape => exit(0),
                                Key::Space => {
                                    if let Err(e) = open::that(store.current_image_path.path.clone()) {
                                        println!("Error while opening file: {}", e);
                                    }
                                },
                                _ => {}
                            }
                        } else if let Event::PointerButton {
                            button, pressed, ..
                        } = e
                        {
                            if *pressed && *button == PointerButton::Secondary {
                                reset_transform = true;
                            }
                        }
                    });
                }
                if updated_image {
                    self.update_texture(false);
                }
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::Resized(new_size) => {
                self.handle_resized(new_size.width, new_size.height);
            }
            _ => (),
        }
    }
}

pub struct ImflowEguiLoader {
    store: Arc<RwLock<ImageStore>>,
    cache: egui::mutex::Mutex<HashMap<String, ImageLoadResult>>,
}

impl ImflowEguiLoader {
    pub fn new(store: Arc<RwLock<ImageStore>>) -> ImflowEguiLoader {
        ImflowEguiLoader {
            store,
            cache: egui::mutex::Mutex::new(HashMap::new()),
        }
    }
}

impl ImageLoader for ImflowEguiLoader {
    fn id(&self) -> &str {
        "ImflowEguiLoader"
    }

    fn load(
        &self,
        _ctx: &egui::Context,
        uri: &str,
        _size_hint: egui::SizeHint,
    ) -> egui::load::ImageLoadResult {
        let mut cache = self.cache.lock();

        // let id = uri.parse::<usize>().unwrap();
        let id = uri.to_string();
        if let Some(handle) = cache.get(&id) {
            handle.clone()
        } else {
            let imbuf = {
                let binding = self.store.read().unwrap();
                binding.get_thumbnail_hash(id.clone()).clone()
            };
            let mut image = ColorImage::new([imbuf.width, imbuf.height], Color32::BLACK);
            let image_buffer = image.as_raw_mut();
            println!(
                "w: {} h: {} len: {}",
                imbuf.width,
                imbuf.height,
                imbuf.rgba_buffer.len()
            );
            for (i, &value) in imbuf.rgba_buffer.iter().enumerate() {
                let bytes = value.to_le_bytes();
                let start = i * 4;
                image_buffer[start..start + 4].copy_from_slice(&bytes);
            }

            let res = ImageLoadResult::Ok(egui::load::ImagePoll::Ready {
                image: Arc::new(ColorImage {
                    size: [imbuf.width, imbuf.height],
                    pixels: image.pixels,
                }),
            });
            cache.insert(id, res.clone());
            res.clone()
        }
    }

    // TODO
    fn forget(&self, _uri: &str) {}

    // TODO
    fn forget_all(&self) {}

    // TODO
    fn byte_size(&self) -> usize {
        todo!()
    }
}
