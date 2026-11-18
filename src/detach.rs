use std::fs::{create_dir_all, OpenOptions};
use std::io::{stderr, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

pub fn log_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("arch-control-center").join("log.txt")
}

/// Fork into a new session and send stdout/stderr to the cache log.
/// The parent prints the log path and exits so the launching shell returns.
pub fn detach() {
    let path = log_path();
    if let Some(dir) = path.parent() {
        if let Err(err) = create_dir_all(dir) {
            let _ = writeln!(stderr(), "arch-control-center: не удалось создать {}: {err}", dir.display());
            return;
        }
    }
    let _ = writeln!(
        stderr(),
        "arch-control-center: процесс отсоединён, лог {}",
        path.display()
    );
    let _ = stderr().flush();

    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            let _ = writeln!(stderr(), "arch-control-center: fork не удался, остаюсь в терминале");
            return;
        }
        if pid > 0 {
            std::process::exit(0);
        }
        let _ = libc::setsid();
        let pid2 = libc::fork();
        if pid2 < 0 {
            return;
        }
        if pid2 > 0 {
            std::process::exit(0);
        }
    }

    redirect(&path);
    let _ = writeln!(
        std::io::stdout(),
        "\n--- arch-control-center {} ---",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
}

fn redirect(path: &Path) {
    let mut opts = OpenOptions::new();
    opts.create(true).write(true);
    if std::fs::metadata(path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        opts.truncate(true);
    } else {
        opts.append(true);
    }
    let Ok(file) = opts.open(path) else {
        return;
    };
    let fd = file.as_raw_fd();
    unsafe {
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY);
        if devnull >= 0 {
            libc::dup2(devnull, 0);
            libc::close(devnull);
        }
        libc::dup2(fd, 1);
        libc::dup2(fd, 2);
    }
}
