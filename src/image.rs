use zune_image::codecs::qoi::zune_core::options::DecoderOptions;

use std::fs::File;

pub enum Approach {
    Mmap,
    Path,
    Iced,
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
pub fn load_thumbnail(
    path: &str,
    approach: Approach,
) -> Result<iced::widget::image::Handle, String> {
    match approach {
        Approach::Mmap => {
            let file = File::open(path).map_err(|e| e.to_string())?;
            let mmap = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| e.to_string())?;
            println!("mapped file");
            let img = zune_image::image::Image::read(&*mmap, DecoderOptions::default()).unwrap();
            let width = img.dimensions().0 as u32;
            let height = img.dimensions().1 as u32;
            println!("loaded");
            let flat = img.flatten_to_u8();
            println!("flattened");
            let rgba = convert_rgb_to_rgba(flat);
            println!("rgbad");
            let conv = iced::widget::image::Handle::from_rgba(width, height, rgba);
            println!("iced");

            Ok(conv)
        }
        Approach::Path => {
            let img = zune_image::image::Image::open_with_options(path, DecoderOptions::default())
                .unwrap();
            let width = img.dimensions().0 as u32;
            let height = img.dimensions().1 as u32;
            println!("loaded");
            let flat = img.flatten_to_u8();
            println!("flattened");
            let rgba = convert_rgb_to_rgba(flat);
            println!("rgbad");
            let conv = iced::widget::image::Handle::from_rgba(width, height, rgba);
            println!("iced");

            Ok(conv)
        }
        Approach::Iced => {
            let file = File::open(path).map_err(|e| e.to_string())?;
            let mmap = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| e.to_string())?;
            let img = image::load_from_memory(&mmap).map_err(|e| e.to_string())?;
            let width = img.width();
            let height = img.height();
            println!("loaded");
            let rgba = img.into_rgba8().into_raw();
            println!("rgbad");
            let conv = iced::widget::image::Handle::from_rgba(width, height, rgba);
            println!("iced");

            Ok(conv)
        }
    }
}
