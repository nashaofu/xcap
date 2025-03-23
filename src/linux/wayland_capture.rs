use dbus::{
    arg::{AppendAll, Iter, IterAppend, PropMap, ReadAll, RefArg, TypeMismatchError, Variant},
    blocking::Connection,
    message::{MatchRule, SignalArgs},
};
use image::RgbaImage;
use percent_encoding::percent_decode;
use std::{
    collections::HashMap,
    env::temp_dir,
    fs::{self},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::error::{XCapError, XCapResult};

use super::utils::png_to_rgba_image;

#[derive(Debug)]
struct OrgFreedesktopPortalRequestResponse {
    status: u32,
    results: PropMap,
}

impl AppendAll for OrgFreedesktopPortalRequestResponse {
    fn append(&self, i: &mut IterAppend) {
        RefArg::append(&self.status, i);
        RefArg::append(&self.results, i);
    }
}

impl ReadAll for OrgFreedesktopPortalRequestResponse {
    fn read(i: &mut Iter) -> Result<Self, TypeMismatchError> {
        Ok(OrgFreedesktopPortalRequestResponse {
            status: i.read()?,
            results: i.read()?,
        })
    }
}

impl SignalArgs for OrgFreedesktopPortalRequestResponse {
    const NAME: &'static str = "Response";
    const INTERFACE: &'static str = "org.freedesktop.portal.Request";
}

fn org_gnome_shell_screenshot(
    conn: &Connection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let proxy = conn.with_proxy(
        "org.gnome.Shell.Screenshot",
        "/org/gnome/Shell/Screenshot",
        Duration::from_secs(10),
    );

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?;

    let dirname = temp_dir().join("screenshot");

    fs::create_dir_all(&dirname)?;

    let mut path = dirname.join(timestamp.as_micros().to_string());
    path.set_extension("png");

    let filename = path.to_string_lossy().to_string();

    proxy.method_call::<(), (i32, i32, i32, i32, bool, &String), &str, &str>(
        "org.gnome.Shell.Screenshot",
        "ScreenshotArea",
        (x, y, width, height, false, &filename),
    )?;

    let rgba_image = png_to_rgba_image(&filename, 0, 0, width, height)?;

    fs::remove_file(&filename)?;

    Ok(rgba_image)
}

fn org_freedesktop_portal_screenshot(
    conn: &Connection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let status: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let status_res = status.clone();
    let path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let path_res = path.clone();

    let match_rule = MatchRule::new_signal("org.freedesktop.portal.Request", "Response");
    conn.add_match(
        match_rule,
        move |response: OrgFreedesktopPortalRequestResponse, _conn, _msg| {
            if let Ok(mut status) = status.lock() {
                *status = Some(response.status);
            }

            let uri = response.results.get("uri").and_then(|str| str.as_str());
            if let (Some(uri_str), Ok(mut path)) = (uri, path.lock()) {
                *path = uri_str[7..].to_string();
            }

            true
        },
    )?;

    let proxy = conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(10000),
    );

    let mut options: PropMap = HashMap::new();
    options.insert(
        String::from("handle_token"),
        Variant(Box::new(String::from("1234"))),
    );
    options.insert(String::from("modal"), Variant(Box::new(true)));
    options.insert(String::from("interactive"), Variant(Box::new(false)));

    proxy.method_call::<(), (&str, PropMap), &str, &str>(
        "org.freedesktop.portal.Screenshot",
        "Screenshot",
        ("", options),
    )?;

    // wait 60 seconds for user interaction
    for _ in 0..60 {
        let result = conn.process(Duration::from_millis(1000))?;
        let status = status_res
            .lock()
            .map_err(|_| XCapError::new("Get status lock failed"))?;

        if result && status.is_some() {
            break;
        }
    }

    let status = status_res
        .lock()
        .map_err(|_| XCapError::new("Get status lock failed"))?;
    let status = *status;

    let path = path_res
        .lock()
        .map_err(|_| XCapError::new("Get path lock failed"))?;
    let path = &*path;

    if status.ne(&Some(0)) || path.is_empty() {
        if !path.is_empty() {
            fs::remove_file(path)?;
        }
        return Err(XCapError::new("Screenshot failed or canceled"));
    }

    let filename = percent_decode(path.as_bytes())
        .decode_utf8()
        .map_err(XCapError::new)?
        .to_string();
    let rgba_image = png_to_rgba_image(&filename, x, y, width, height)?;

    fs::remove_file(&filename)?;

    Ok(rgba_image)
}

static DBUS_LOCK: Mutex<()> = Mutex::new(());

fn wlroots_screenshot(
    x_coordinate: i32,
    y_coordinate: i32,
    width: i32,
    height: i32,
) -> XCapResult<RgbaImage> {
    let wayshot_connection = libwayshot::WayshotConnection::new()?;
    let capture_region = libwayshot::CaptureRegion {
        x_coordinate,
        y_coordinate,
        width,
        height,
    };
    let rgba_image = wayshot_connection.screenshot(capture_region, false)?;

    // libwayshot returns image 0.24 RgbaImage
    // we need image 0.25 RgbaImage
    let image = image::RgbaImage::from_raw(
        rgba_image.width(),
        rgba_image.height(),
        rgba_image.into_raw(),
    )
    .expect("Conversion of PNG -> Raw -> PNG does not fail");

    Ok(image)
}

pub fn wayland_capture(x: i32, y: i32, width: i32, height: i32) -> XCapResult<RgbaImage> {
    let lock = DBUS_LOCK.lock();

    let conn = Connection::new_session()?;
    let res = org_gnome_shell_screenshot(&conn, x, y, width, height)
        .or_else(|_| org_freedesktop_portal_screenshot(&conn, x, y, width, height))
        .or_else(|_| wlroots_screenshot(x, y, width, height));

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
