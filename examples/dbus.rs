use dbus::{
  arg::{AppendAll, Iter, IterAppend, PropMap, ReadAll, RefArg, TypeMismatchError, Variant},
  blocking::Connection,
  channel::MatchingReceiver,
  message::{MatchRule, SignalArgs},
  Path,
};
use std::{
  collections::HashMap,
  fs, path,
  sync::{Arc, Mutex},
  time::Duration,
};

#[derive(Debug)]
pub struct OrgFreedesktopPortalRequestResponse {
  pub status: u32,
  pub results: PropMap,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let conn = Connection::new_session()?;

  let match_rule = MatchRule::new_signal("org.freedesktop.portal.Request", "Response");

  let path: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
  let path_res = path.clone();

  let token = conn.add_match(
    match_rule,
    move |response: OrgFreedesktopPortalRequestResponse, _conn, _msg| {
      let uri = response.results.get("uri").and_then(|str| str.as_str());
      if let (Some(uri_str), Ok(mut path)) = (uri, path.lock()) {
        *path = Some(uri_str[7..].to_string());
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

  let _: (Path<'static>,) = proxy.method_call(
    "org.freedesktop.portal.Screenshot",
    "Screenshot",
    ("", options),
  )?;

  // wait 3 minutes for user interaction
  loop {
    let result = conn.process(Duration::from_millis(100))?;
    let path = path_res.lock().unwrap();
    if result && path.is_some() {
      conn.stop_receive(token);
      break;
    }
  }
  
  let path = path_res.lock().unwrap().clone().unwrap();
  let buffer = fs::read(path)?;
  println!("buffer {}", buffer.len());

  Ok(())
}
