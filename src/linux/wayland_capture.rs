use std::{env::temp_dir, fs, sync::Mutex};

use ashpd::{
    desktop::screenshot::Screenshot,
    zbus::{self, Connection},
};
use image::RgbaImage;
use scopeguard::defer;

use super::wayland_output::desktop_bounds;
use crate::{
    error::{XCapError, XCapResult},
    platform::utils::safe_uri_to_path,
};

async fn org_gnome_shell_screenshot(
    conn: &Connection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.gnome.Shell.Screenshot",
        "/org/gnome/Shell/Screenshot",
        "org.gnome.Shell.Screenshot",
    )
    .await
    .map_err(|e| XCapError::new(e.to_string()))?;

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
    let reply = proxy
        .call_method("ScreenshotArea", &(x, y, width, height, false, &filename))
        .await
        .map_err(|e| XCapError::new(e.to_string()))?;
    let (success, _): (bool, String) = reply.body().deserialize().map_err(XCapError::new)?;
    if !success {
        return Err(XCapError::new("GNOME ScreenshotArea failed"));
    }
    // ScreenshotArea already cropped the region, at the output's pixel scale.
    Ok(image::open(&filename)?.to_rgba8())
}

/// https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html
async fn org_freedesktop_portal_screenshot(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let screenshot_response = Screenshot::request()
        .interactive(false)
        .send()
        .await
        .map_err(|e| XCapError::new(e.to_string()))?
        .response()
        .map_err(|e| XCapError::new(e.to_string()))?;

    let filename = safe_uri_to_path(screenshot_response.uri().as_str())?;

    defer!({
        let _ = fs::remove_file(&filename);
    });

    let image = image::open(&filename)?;
    let (left, top, right, bottom) = desktop_bounds()?;
    let scale_x = f64::from(image.width()) / (right - left) as f64;
    let scale_y = f64::from(image.height()) / (bottom - top) as f64;
    // The portal must return a uniformly scaled desktop; a selected output is ambiguous.
    if (scale_x - scale_y).abs() > scale_x.max(scale_y) * 0.001
        || i64::from(x) < left
        || i64::from(y) < top
        || i64::from(x) + i64::from(width) > right
        || i64::from(y) + i64::from(height) > bottom
    {
        return Err(XCapError::new(
            "Portal screenshot does not match the desktop layout",
        ));
    }
    let crop_x = ((i64::from(x) - left) as f64 * scale_x).round() as u32;
    let crop_y = ((i64::from(y) - top) as f64 * scale_y).round() as u32;
    let crop_right = ((i64::from(x) + i64::from(width) - left) as f64 * scale_x).round() as u32;
    let crop_bottom = ((i64::from(y) + i64::from(height) - top) as f64 * scale_y).round() as u32;
    Ok(image
        .crop_imm(crop_x, crop_y, crop_right - crop_x, crop_bottom - crop_y)
        .to_rgba8())
}

fn wlroots_screenshot(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    use libwayshot_xcap::{
        WayshotConnection,
        region::{LogicalRegion, Position, Region, Size},
    };
    let connection = WayshotConnection::new()?;
    let image = connection.screenshot(
        LogicalRegion {
            inner: Region {
                position: Position { x, y },
                size: Size {
                    width: width as u32,
                    height: height as u32,
                },
            },
        },
        false,
    )?;
    Ok(image.to_rgba8())
}

static DBUS_LOCK: Mutex<()> = Mutex::new(());

pub fn wayland_capture(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    if width <= 0 || height <= 0 {
        return Err(XCapError::InvalidCaptureRegion(
            "Capture dimensions must be positive".into(),
        ));
    }
    let lock = DBUS_LOCK.lock()?;

    let res = pollster::block_on(async {
        let conn = zbus::Connection::session()
            .await
            .map_err(|e| XCapError::new(e.to_string()))?;
        match org_gnome_shell_screenshot(&conn, x, y, width, height).await {
            Ok(img) => Ok(img),
            Err(e) => {
                log::debug!("org_gnome_shell_screenshot failed {e}");
                match org_freedesktop_portal_screenshot(x, y, width, height).await {
                    Ok(image) => Ok(image),
                    Err(e) => {
                        log::debug!("org_freedesktop_portal_screenshot failed {e}");
                        Err(e)
                    }
                }
            }
        }
    })
    .or_else(|err| {
        log::debug!("D-Bus screenshot failed: {err}");
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
