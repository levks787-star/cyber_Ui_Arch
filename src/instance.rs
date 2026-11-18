//! One overlay at a time. A second `--overlay` asks the first to close and exits.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

pub struct OverlayLock {
    close: Arc<AtomicBool>,
    path: PathBuf,
}

impl OverlayLock {
    /// Bind the toggle socket, or signal the running overlay and exit.
    pub fn acquire_or_exit() -> Self {
        let path = socket_path();
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(b"close");
            let _ = stream.flush();
            log::info!("overlay already open, asking it to close");
            std::process::exit(0);
        }
        let _ = std::fs::remove_file(&path);
        let close = Arc::new(AtomicBool::new(false));
        match UnixListener::bind(&path) {
            Ok(listener) => {
                let flag = Arc::clone(&close);
                thread::Builder::new()
                    .name("acc-overlay".into())
                    .spawn(move || {
                        for mut stream in listener.incoming().flatten() {
                            let mut buf = [0u8; 8];
                            let _ = stream.read(&mut buf);
                            flag.store(true, Ordering::SeqCst);
                        }
                    })
                    .ok();
            }
            Err(err) => log::warn!("overlay socket {}: {err}", path.display()),
        }
        Self { close, path }
    }

    pub fn close_requested(&self) -> bool {
        self.close.swap(false, Ordering::SeqCst)
    }
}

impl Drop for OverlayLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("arch-control-center.sock");
        }
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    PathBuf::from(format!("/tmp/arch-control-center-{user}.sock"))
}
