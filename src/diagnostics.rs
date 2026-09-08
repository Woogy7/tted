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

/// Aggregate UI timings without recording document contents or pressed keys.
pub(crate) struct Performance {
    since: std::time::Instant,
    frames: u64,
    render_us: u128,
    render_max_us: u128,
    events: u64,
    input_us: u128,
    input_max_us: u128,
}

impl Default for Performance {
    fn default() -> Self {
        Self {
            since: std::time::Instant::now(),
            frames: 0,
            render_us: 0,
            render_max_us: 0,
            events: 0,
            input_us: 0,
            input_max_us: 0,
        }
    }
}
impl Performance {
    pub fn render(&mut self, elapsed: std::time::Duration) {
        self.frames += 1;
        self.render_us += elapsed.as_micros();
        self.render_max_us = self.render_max_us.max(elapsed.as_micros());
    }
    pub fn input(&mut self, elapsed: std::time::Duration) {
        self.events += 1;
        self.input_us += elapsed.as_micros();
        self.input_max_us = self.input_max_us.max(elapsed.as_micros());
    }
    pub fn tick(&mut self) {
        if self.since.elapsed() >= std::time::Duration::from_secs(5) {
            self.flush();
        }
    }
    fn flush(&mut self) {
        if self.frames != 0 || self.events != 0 {
            log(&format!("performance frames={} render_avg_us={} render_max_us={} input_events={} input_avg_us={} input_max_us={}", self.frames, self.render_us / u128::from(self.frames.max(1)), self.render_max_us, self.events, self.input_us / u128::from(self.events.max(1)), self.input_max_us));
        }
        self.since = std::time::Instant::now();
        self.frames = 0;
        self.render_us = 0;
        self.render_max_us = 0;
        self.events = 0;
        self.input_us = 0;
        self.input_max_us = 0;
    }
}
impl Drop for Performance {
    fn drop(&mut self) {
        self.flush();
    }
}
