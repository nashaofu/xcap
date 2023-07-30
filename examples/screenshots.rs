use png::Compression;
use screenshots::Screen;
use std::{fs, time::Instant};

fn main() {
    let start = Instant::now();
    let screens = Screen::all().unwrap();

    for screen in screens {
        println!("capturer {screen:?}");
        let mut image = screen.capture().unwrap();
        let mut buffer = image.to_png(None).unwrap();
        let mut compressed_buffer = image.to_png(Some(Compression::Best)).unwrap();
        fs::write(format!("target/{}.png", screen.display_info.id), buffer).unwrap();
        fs::write(
            format!("target/{}-compressed.png", screen.display_info.id),
            compressed_buffer,
        )
        .unwrap();

        image = screen.capture_area(300, 300, 300, 300).unwrap();
        buffer = image.to_png(None).unwrap();
        compressed_buffer = image.to_png(Some(Compression::Best)).unwrap();

        fs::write(format!("target/{}-2.png", screen.display_info.id), buffer).unwrap();

        fs::write(
            format!("target/{}-2-compressed.png", screen.display_info.id),
            compressed_buffer,
        )
        .unwrap();
    }

    let screen = Screen::from_point(100, 100).unwrap();
    println!("capturer {screen:?}");

    let image = screen.capture_area(300, 300, 300, 300).unwrap();
    let buffer = image.to_png(None).unwrap();
    let compressed_buffer = image.to_png(Some(Compression::Best)).unwrap();

    fs::write("target/capture_display_with_point.png", buffer).unwrap();
    fs::write("target/capture_display_with_point.png", compressed_buffer).unwrap();
    println!("运行耗时: {:?}", start.elapsed());
}
