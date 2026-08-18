use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

struct Logger {
    path: PathBuf,
    file: Mutex<File>,
}

static LOGGER: OnceLock<Option<Logger>> = OnceLock::new();

pub fn init() -> Option<PathBuf> {
    let logger = LOGGER.get_or_init(|| {
        let path = std::env::var_os("TTED_LOG").map_or_else(
            || std::env::temp_dir().join(format!("tted-{}.log", std::process::id())),
            PathBuf::from,
        );
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
            .map(|file| Logger {
                path,
                file: Mutex::new(file),
            })
    });
    log("diagnostics initialized");
    logger.as_ref().map(|logger| logger.path.clone())
}

pub fn log(message: &str) {
    let Some(logger) = LOGGER.get().and_then(Option::as_ref) else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    if let Ok(mut file) = logger.file.lock() {
        let _ = writeln!(file, "{timestamp} {message}");
        let _ = file.flush();
    }
}
