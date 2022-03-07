use display_info::{get_display_info_from_point, get_display_infos, DisplayInfo};

#[derive(Debug, Clone, Copy)]
pub struct Capturer {
  pub display_info: DisplayInfo,
}

impl Capturer {
  pub fn new(display_info: DisplayInfo) -> Self {
    Capturer { display_info }
  }

  pub fn screen_capturers() -> Vec<Capturer> {
    get_display_infos()
      .iter()
      .map(move |display_info| Capturer::new(*display_info))
      .collect()
  }

  pub fn screen_capturer_from_point(x: i32, y: i32) -> Option<Capturer> {
    match get_display_info_from_point(x, y) {
      Some(display_info) => Some(Capturer::new(display_info)),
      None => None,
    }
  }
}
