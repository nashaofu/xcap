use dbus::{
    arg::{self, PropMap, Variant},
    blocking::Connection,
    Message, Path,
  };
  use std::{collections::HashMap, time::Duration};
  
  #[derive(Debug)]
  pub struct ComExampleDbustestHelloHappened {
    pub sender: String,
  }
  
  impl arg::AppendAll for ComExampleDbustestHelloHappened {
    fn append(&self, i: &mut arg::IterAppend) {
      arg::RefArg::append(&self.sender, i);
    }
  }
  
  impl arg::ReadAll for ComExampleDbustestHelloHappened {
    fn read(i: &mut arg::Iter) -> Result<Self, arg::TypeMismatchError> {
      Ok(ComExampleDbustestHelloHappened { sender: i.read()? })
    }
  }
  
  impl dbus::message::SignalArgs for ComExampleDbustestHelloHappened {
    const NAME: &'static str = "Response";
    const INTERFACE: &'static str = "org.freedesktop.portal.Desktop";
  }
  
  fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::new_session()?;
  
    {
      let proxy = conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(10000),
      );
  
      // Let's start listening to signals.
      let _id = proxy
        .match_signal(
          |h: ComExampleDbustestHelloHappened, _: &Connection, _: &Message| {
            println!("sender: {}", h.sender);
            true
          },
        )
        .unwrap();
  
      let mut options: PropMap = HashMap::new();
      options.insert(
        String::from("handle_token"),
        Variant(Box::new(String::from("1234"))),
      );
  
      options.insert(String::from("modal"), Variant(Box::new(true)));
      options.insert(String::from("interactive"), Variant(Box::new(false)));
  
      let r: (Path,) = proxy.method_call(
        "org.freedesktop.portal.Screenshot",
        "Screenshot",
        ("", options),
      )?;
  
      println!("ok {:?}", r);
    }
  
    // Listen to incoming signals forever.
    loop {
      conn.process(Duration::from_millis(100000)).unwrap();
    }
  }
  