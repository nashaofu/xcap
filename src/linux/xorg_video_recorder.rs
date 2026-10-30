use std::{
    sync::{
        Arc,
        mpsc::{Receiver, TrySendError, sync_channel},
    },
    thread,
    time::Duration,
};

use super::impl_monitor::ImplMonitor;
use crate::{
    XCapResult,
    video_recorder::{Frame, RecorderWaker, RecorderWorker},
};

#[derive(Debug, Clone)]
pub struct XorgVideoRecorder {
    worker: Arc<RecorderWorker>,
}

impl XorgVideoRecorder {
    pub fn new(monitor: ImplMonitor) -> XCapResult<(Self, Receiver<Frame>)> {
        let (sender, receiver) = sync_channel(2);
        let waker = Arc::new(RecorderWaker::new());
        let thread_waker = waker.clone();
        let handle = thread::spawn(move || {
            let result = (|| -> XCapResult<()> {
                while thread_waker.wait()? {
                    match monitor.capture_image() {
                        Ok(image) => {
                            let frame = Frame::new(image.width(), image.height(), image.into_raw());
                            match sender.try_send(frame) {
                                Ok(()) | Err(TrySendError::Full(_)) => {}
                                Err(TrySendError::Disconnected(_)) => break,
                            }
                        }
                        Err(err) => {
                            log::error!("Failed to capture frame: {err:?}");
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })();
            thread_waker.terminate();
            if let Err(err) = result {
                log::error!("Recorder worker failed: {err}");
            }
        });
        Ok((
            Self {
                worker: Arc::new(RecorderWorker {
                    waker,
                    handle: Some(handle),
                }),
            },
            receiver,
        ))
    }

    pub fn start(&self) -> XCapResult<()> {
        self.worker.waker.wake()
    }

    pub fn stop(&self) -> XCapResult<()> {
        self.worker.waker.sleep()
    }
}
