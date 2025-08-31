use std::ops::{Add, BitAnd};

pub(crate) fn vec_u8_to_u32(buffer: Vec<u8>) -> Vec<u32> {
    let rgba_buffer = unsafe {
        Vec::from_raw_parts(
            buffer.as_ptr() as *mut u32,
            buffer.len() / 4,
            buffer.len() / 4,
        )
    };
    std::mem::forget(buffer);
    rgba_buffer
}

pub(crate) fn vec_u32_to_u8(buffer: Vec<u32>) -> Vec<u8> {
    let rgba_buffer = unsafe {
        Vec::from_raw_parts(
            buffer.as_ptr() as *mut u8,
            buffer.len() * 4,
            buffer.len() * 4,
        )
    };
    std::mem::forget(buffer);
    rgba_buffer
}

pub(crate) fn slice_u8_to_u32(rgba_buffer: &[u8]) -> &[u32] {
    unsafe { std::slice::from_raw_parts(rgba_buffer.as_ptr() as *const u32, rgba_buffer.len() / 4) }
}

pub(crate) fn round_to_4_multiple(value: u32) -> u32 {
    (value + 3) & !3
}
