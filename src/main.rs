use clap::Parser;
use ffmpeg_next as ffmpeg;
use imflow::store::AppEvent;
use std::{env, path::PathBuf};

mod app;
mod egui_tools;

use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;

fn main() {
    if env::var("RUST_LOG").is_err() {
        unsafe { env::set_var("RUST_LOG", "error,imflow=debug") }
    }
    env_logger::init();
    rexiv2::initialize().expect("Failed to initialize rexiv2");
    ffmpeg::init().expect("Failed to initialize ffmpeg");
    ffmpeg::log::set_level(ffmpeg_next::log::Level::Quiet);

    let args = Args::parse();
    let path = args.path.unwrap_or("./test_images".into());
    #[cfg(not(target_arch = "wasm32"))]
    {
        pollster::block_on(run(path));
    }
}

async fn run(path: PathBuf) {
    let event_loop = EventLoop::<AppEvent>::with_user_event().build().unwrap();

    event_loop.set_control_flow(ControlFlow::Wait);
    let event_loop_proxy = event_loop.create_proxy();

    let mut app = App::new(path, event_loop_proxy);

    event_loop.run_app(&mut app).expect("Failed to run app");
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    path: Option<PathBuf>,
}
