use std::{collections::HashMap, env::temp_dir, fmt::Debug, fs, sync::Mutex};

use image::RgbaImage;
use scopeguard::defer;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{DeserializeDict, Type, Value},
};

use crate::{
    error::XCapResult,
    platform::utils::{get_zbus_portal_request, safe_uri_to_path, wait_zbus_response},
};

use super::utils::{get_zbus_connection, png_to_rgba_image};

fn org_gnome_shell_screenshot(
    conn: &Connection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Shell.Screenshot",
        "/org/gnome/Shell/Screenshot",
        "org.gnome.Shell.Screenshot",
    )?;

    let filename = rand::random::<u32>();

    let dirname = temp_dir().join("screenshot");
    fs::create_dir_all(&dirname)?;

    let mut path = dirname.join(filename.to_string());
    path.set_extension("png");
    defer!({
        let _ = fs::remove_file(&path);
    });

    let filename = path.to_string_lossy().to_string();

    // https://github.com/vinzenz/gnome-shell/blob/master/data/org.gnome.Shell.Screenshot.xml
    proxy.call_method("ScreenshotArea", &(x, y, width, height, false, &filename))?;

    let rgba_image = png_to_rgba_image(&filename, 0, 0, width, height)?;

    Ok(rgba_image)
}

#[derive(DeserializeDict, Type, Debug)]
#[zvariant(signature = "dict")]
pub struct ScreenshotResponse {
    uri: String,
}

/// https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html
fn org_freedesktop_portal_screenshot(
    conn: &Connection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let proxy = Proxy::new(
        conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Screenshot",
    )?;

    let handle_token = rand::random::<u32>().to_string();
    let portal_request = get_zbus_portal_request(conn, &handle_token)?;

    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(&handle_token));
    options.insert("modal", Value::from(true));
    options.insert("interactive", Value::from(false));

    // https://github.com/flatpak/xdg-desktop-portal/blob/main/data/org.freedesktop.portal.Screenshot.xml
    proxy.call_method("Screenshot", &("", options))?;
    let screenshot_response: ScreenshotResponse = wait_zbus_response(&portal_request)?;
    let filename = safe_uri_to_path(&screenshot_response.uri)?;
    defer!({
        let _ = fs::remove_file(&filename);
    });

    let rgba_image = png_to_rgba_image(&filename, x, y, width, height)?;

    Ok(rgba_image)
}

static DBUS_LOCK: Mutex<()> = Mutex::new(());

fn wlroots_screenshot(
    x_coordinate: i32,
    y_coordinate: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let wayshot_connection = libwayshot_xcap::WayshotConnection::new()?;
    let capture_region = libwayshot_xcap::region::LogicalRegion {
        inner: libwayshot_xcap::region::Region {
            position: libwayshot_xcap::region::Position {
                x: x_coordinate,
                y: y_coordinate,
            },
            size: libwayshot_xcap::region::Size {
                width: width as u32,
                height: height as u32,
            },
        },
    };
    let rgba_image = wayshot_connection.screenshot(capture_region, false)?;

    // libwayshot returns image 0.24 RgbaImage
    // we need image 0.25 RgbaImage
    let image = image::RgbaImage::from_raw(
        rgba_image.width(),
        rgba_image.height(),
        rgba_image.to_rgba8().into_vec(),
    )
    .expect("Conversion of PNG -> Raw -> PNG does not fail");

    Ok(image)
}

pub fn wayland_capture(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    let lock = DBUS_LOCK.lock();

    let conn = get_zbus_connection()?;
    let res = org_gnome_shell_screenshot(conn, x, y, width, height)
        .or_else(|e| {
            log::debug!("org_gnome_shell_screenshot failed {e}");

            org_freedesktop_portal_screenshot(conn, x, y, width, height)
        })
        .or_else(|e| {
            log::debug!("org_freedesktop_portal_screenshot failed {e}");
            wlroots_screenshot(x, y, width, height)
        });

    drop(lock);

    res
}
#[test]
fn screnshot_multithreaded() {
    fn make_screenshots() {
        let monitors = crate::monitor::Monitor::all().unwrap();
        for monitor in monitors {
            monitor.capture_image().unwrap();
        }
    }
    // Try making screenshots in paralel. If this times out, then this means that there is a threading issue.
    const PARALELISM: usize = 10;
    let handles: Vec<_> = (0..PARALELISM)
        .map(|_| {
            std::thread::spawn(|| {
                make_screenshots();
            })
        })
        .collect();
    make_screenshots();
    handles
        .into_iter()
        .for_each(|handle| handle.join().unwrap());
}
