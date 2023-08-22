use anyhow::{anyhow, Result};
use image::RgbaImage;

pub fn vec_to_rgba_image(width: u32, height: u32, buf: Vec<u8>) -> Result<RgbaImage> {
    RgbaImage::from_vec(width, height, buf).ok_or(anyhow!("buffer not big enough"))
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
pub fn bgra_to_rgba_image(width: u32, height: u32, buf: Vec<u8>) -> Result<RgbaImage> {
    // convert to rgba
    let rgba_buf = buf
        .chunks_exact(4)
        .take((width * height) as usize)
        .map(|bgra| [bgra[2], bgra[1], bgra[0], bgra[3]])
        .flatten()
        .collect();

    vec_to_rgba_image(width, height, rgba_buf)
}

/// Some platforms e.g. MacOS can have extra bytes at the end of each row.
///
/// See
/// https://github.com/nashaofu/screenshots-rs/issues/29
/// https://github.com/nashaofu/screenshots-rs/issues/38
#[cfg(any(target_os = "macos", test))]
pub fn remove_extra_data(width: usize, bytes_per_row: usize, buf: Vec<u8>) -> Vec<u8> {
    buf.chunks_exact(bytes_per_row)
        .map(|row| row.split_at(width * 4).0.to_owned())
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra() {
        let image = bgra_to_rgba_image(2, 1, vec![1, 2, 3, 4, 255, 254, 253, 252]).unwrap();
        assert_eq!(
            image,
            RgbaImage::from_vec(2, 1, vec![3, 2, 1, 4, 253, 254, 255, 252]).unwrap()
        );
    }

    #[test]
    fn extra_data() {
        let clean = remove_extra_data(
            2,
            9,
            vec![
                1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ],
        );
        assert_eq!(
            clean,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15, 16, 17, 18]
        );
    }
}
