use crate::error::Error;
use core::slice;
use std::ffi::c_char;
use vst3::Steinberg::TUID;

pub trait ToRustString {
    fn to_rust_string(&self) -> String;
}

impl<'a, const N: usize> ToRustString for &'a [i8; N] {
    fn to_rust_string(&self) -> String {
        let len = self.iter().position(|ch| *ch == 0).unwrap_or(N);
        let slice = unsafe { slice::from_raw_parts(self.as_ptr().cast(), len) };
        String::from_utf8_lossy(slice).to_string()
    }
}

impl<'a, const N: usize> ToRustString for &'a [i16; N] {
    fn to_rust_string(&self) -> String {
        let len = self.iter().position(|ch| *ch == 0).unwrap_or(128);
        let slice = unsafe { slice::from_raw_parts(self.as_ptr().cast(), len) };
        String::from_utf16_lossy(slice)
    }
}

/// Helper to parse a string into a TUID.
pub fn parse_class_id(s: &str) -> Result<TUID, Error> {
    fn parse_hexit(ch: u8) -> Result<c_char, Error> {
        match ch {
            b'0' => Ok(0x00),
            b'1' => Ok(0x01),
            b'2' => Ok(0x02),
            b'3' => Ok(0x03),
            b'4' => Ok(0x04),
            b'5' => Ok(0x05),
            b'6' => Ok(0x06),
            b'7' => Ok(0x07),
            b'8' => Ok(0x08),
            b'9' => Ok(0x09),
            b'a' | b'A' => Ok(0x0a),
            b'b' | b'B' => Ok(0x0b),
            b'c' | b'C' => Ok(0x0c),
            b'd' | b'D' => Ok(0x0d),
            b'e' | b'E' => Ok(0x0e),
            b'f' | b'F' => Ok(0x0f),
            _ => Err(Error::InvalidArg),
        }
    }
    let mut cid = [0; 16];
    let mut idx = 0;

    let bytes = s
        .as_bytes()
        .chunks_exact(2)
        .map(|chunk| -> Result<c_char, Error> {
            let hi = parse_hexit(chunk[0])?;
            let lo = parse_hexit(chunk[1])?;
            Ok((hi << 4) | lo)
        });
    for byte in bytes {
        *cid.get_mut(idx).ok_or(Error::InvalidArg)? = byte?;
        idx += 1;
    }
    if idx != cid.len() {
        return Err(Error::InvalidArg);
    }
    Ok(cid)
}

#[cfg(test)]
mod tests {
    use vst3::Steinberg::TUID;

    #[test]
    fn parse_class_id() {
        let cid: TUID = [
            0x41 as _, 0x34 as _, 0x7F as _, 0xD6 as _, 0xFE as _, 0xD6 as _, 0x40 as _, 0x94 as _,
            0xAF as _, 0xBB as _, 0x12 as _, 0xB7 as _, 0xDB as _, 0xA1 as _, 0xD4 as _, 0x41 as _,
        ];
        assert_eq!(
            Ok(cid),
            super::parse_class_id("41347FD6FED64094AFBB12B7DBA1D441")
        );
    }
}
