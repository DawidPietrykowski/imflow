use crate::egui_tools::EguiRenderer;
use egui::{Event, Key, PointerButton};
use egui_wgpu::wgpu::SurfaceError;
use egui_wgpu::{ScreenDescriptor, wgpu};
use imflow::store::ImageStore;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use wgpu::{PipelineCompilationOptions, SurfaceConfiguration};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::platform::x11::WindowAttributesExtX11;
use winit::window::{Window, WindowId};

// Uniforms for transformations
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Transforms {
    transform: [f32; 16], // 4x4 matrix
    width: u32,
    height: u32,
    _padding1: u32,
    _padding2: u32,
}

pub(crate) struct TransformData {
    pan_x: f32,
    pan_y: f32,
    zoom: f32,
    width: u32,
    height: u32,
}

#[rustfmt::skip]
fn create_transform_matrix(data: &TransformData, scale_x: f32, scale_y: f32) -> [f32; 16] {
    const ZOOM_MULTIPLIER: f32 = 3.0;
    let zoom = data.zoom.powf(ZOOM_MULTIPLIER);

    [
        zoom * scale_x, 0.0,            0.0, 0.0,
        0.0,            zoom * scale_y, 0.0, 0.0,
        0.0,            0.0,            1.0, 0.0,
        data.pan_x,     data.pan_y,     0.0, 1.0,
    ]
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
    pub store: ImageStore,
    pub image_texture: wgpu::Texture,
    pub bind_group: wgpu::BindGroup,
    pub render_pipeline: wgpu::RenderPipeline,
    pub transform_buffer: wgpu::Buffer,
    pub transform_data: TransformData,
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
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: features,
                    required_limits: Default::default(),
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

        let egui_renderer = EguiRenderer::new(&device, surface_config.format, None, 1, window);

        let scale_factor = 1.0;

        let store = ImageStore::new(path);

        let (image_texture, bind_group, render_pipeline, transform_buffer) =
            // setup_texture(&device, surface_config.clone(), 6000, 4000);
            setup_texture(&device, surface_config.clone(), 8192, 8192);

        let transform_data = TransformData {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            width: 10000,
            height: 10000,
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
        }
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
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

        self.pan_zoom(0.0, 0.0, 0.0);
        self.update_texture();
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.state.as_mut().unwrap().resize_surface(width, height);
        }
        self.pan_zoom(0.0, 0.0, 0.0);
    }

    pub fn update_texture(&mut self) {
        let state = self.state.as_mut().unwrap();

        state.store.check_loaded_images();
        let imbuf = if let Some(full) = state.store.get_current_image() {
            full
        } else {
            state.store.get_thumbnail()
        };
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

        self.pan_zoom(0.0, 0.0, 0.0);
    }

    fn update_transform(&mut self) {
        let state = self.state.as_mut().unwrap();

        let image_aspect_ratio =
            (state.transform_data.width as f32) / (state.transform_data.height as f32);
        let window_size = self.window.as_ref().unwrap().inner_size();
        let window_aspect_ratio = window_size.width as f32 / window_size.height as f32;
        let mut scale_x = 1.0;
        let mut scale_y = 1.0;
        if window_aspect_ratio > image_aspect_ratio {
            scale_x = image_aspect_ratio / window_aspect_ratio;
        } else {
            scale_y = window_aspect_ratio / image_aspect_ratio;
        }
        let transform = create_transform_matrix(&state.transform_data, scale_x, scale_y);
        state.queue.write_buffer(
            &state.transform_buffer,
            0,
            bytemuck::cast_slice(&[Transforms {
                transform,
                width: state.transform_data.width,
                height: state.transform_data.height,
                _padding1: 0,
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

    pub fn pan_zoom(&mut self, zoom_delta: f32, pan_x: f32, pan_y: f32) {
        let state = self.state.as_mut().unwrap();

        state.transform_data.zoom = (state.transform_data.zoom + zoom_delta).clamp(1.0, 20.0);
        state.transform_data.pan_x += pan_x;
        state.transform_data.pan_y += pan_y;

        self.update_transform();
    }

    fn handle_redraw(&mut self) {
        // Attempt to handle minimizing window
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
                    view: &surface_view,
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
                    view: &surface_view,
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

        let rating = state.store.get_current_rating();
        let path = state.store.current_image_path.clone();
        let filename = path.path.file_name().unwrap();
        let window = self.window.as_ref().unwrap();
        {
            state.egui_renderer.begin_frame(window);

            egui::Window::new("Rating")
                .collapsible(false)
                .resizable(false)
                .default_width(5.0)
                .show(state.egui_renderer.context(), |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.1}", rating))
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
                let (events, _keys_down, pointer) = self
                    .state
                    .as_ref()
                    .unwrap()
                    .egui_renderer
                    .context()
                    .input(|i| (i.events.clone(), i.keys_down.clone(), i.pointer.clone()));

                events.iter().for_each(|e| {
                    if let Event::Key { key, pressed, .. } = e {
                        if !*pressed {
                            return;
                        }
                        match *key {
                            Key::ArrowLeft => {
                                self.state.as_mut().unwrap().store.next_image(-1);
                                self.update_texture();
                            }
                            Key::ArrowRight => {
                                self.state.as_mut().unwrap().store.next_image(1);
                                self.update_texture();
                            }
                            Key::ArrowUp => {
                                let rating =
                                    self.state.as_mut().unwrap().store.get_current_rating();
                                self.state.as_mut().unwrap().store.set_rating(rating + 1);
                            }
                            Key::ArrowDown => {
                                let rating =
                                    self.state.as_mut().unwrap().store.get_current_rating();
                                self.state.as_mut().unwrap().store.set_rating(rating - 1);
                            }
                            Key::Backtick => self.state.as_mut().unwrap().store.set_rating(0),
                            Key::Num0 => self.state.as_mut().unwrap().store.set_rating(0),
                            Key::Num1 => self.state.as_mut().unwrap().store.set_rating(1),
                            Key::Num2 => self.state.as_mut().unwrap().store.set_rating(2),
                            Key::Num3 => self.state.as_mut().unwrap().store.set_rating(3),
                            Key::Num4 => self.state.as_mut().unwrap().store.set_rating(4),
                            Key::Num5 => self.state.as_mut().unwrap().store.set_rating(5),
                            Key::Escape => exit(0),
                            _ => {}
                        }
                    } else if let Event::MouseWheel { delta, .. } = e {
                        self.pan_zoom(delta.y * 0.2, 0.0, 0.0);
                    } else if let Event::PointerButton {
                        button, pressed, ..
                    } = e
                    {
                        if *pressed && *button == PointerButton::Secondary {
                            self.reset_transform();
                        }
                    }
                });

                if pointer.primary_down() && pointer.is_moving() {
                    self.pan_zoom(0.0, pointer.delta().x * 0.001, pointer.delta().y * -0.001);
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
