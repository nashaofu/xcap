use crate::error::{XCapError, XCapResult};

pub(super) fn wide_string_to_string(wide_string: &[u16]) -> XCapResult<String> {
    if let Some(null_pos) = wide_string.iter().position(|pos| *pos == 0) {
        let string = String::from_utf16(&wide_string[..null_pos])?;
        return Ok(string);
    }

    Err(XCapError::new("Convert wide string to string failed"))
}
