use core::slice;
use vst3::Steinberg::{Vst::String128, TUID};
use crate::error::Error;

pub fn string128_to_string(s: &String128) -> String {
    let len = s.iter().position(|ch| *ch == 0).unwrap_or(128);
    let slice = unsafe { slice::from_raw_parts(s.as_ptr().cast(), len) };
    String::from_utf16_lossy(slice)
}

pub fn parse_class_id(s: &str) -> Result<TUID, Error> {
    todo!();
}