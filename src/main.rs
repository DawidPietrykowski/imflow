use clap::Parser;
use std::path::PathBuf;

mod app;
mod egui_tools;

use winit::event_loop::{ControlFlow, EventLoop};

fn main() {
    let args = Args::parse();
    let path = args.path.unwrap_or("./test_images".into());
    #[cfg(not(target_arch = "wasm32"))]
    {
        pollster::block_on(run(path));
    }
}

async fn run(path: PathBuf) {
    let event_loop = EventLoop::new().unwrap();

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = app::App::new(path);

    event_loop.run_app(&mut app).expect("Failed to run app");
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    path: Option<PathBuf>,
}
