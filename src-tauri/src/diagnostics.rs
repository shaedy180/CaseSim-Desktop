use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

// Bounded, local event codes only. Never record URLs, cookies, arguments or tokens.
pub struct Diagnostics(Mutex<PathBuf>);
impl Diagnostics {
    pub fn new(directory: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&directory)?;
        Ok(Self(Mutex::new(directory)))
    }
    pub fn record(&self, event: &str) {
        let Ok(directory) = self.0.lock() else { return };
        let path = directory.join("desktop.log");
        if fs::metadata(&path).is_ok_and(|m| m.len() >= 256 * 1024) {
            // Windows rename does not replace an existing destination.
            let _ = fs::remove_file(directory.join("desktop.3.log"));
            for i in (1..=2).rev() {
                let _ = fs::rename(
                    directory.join(format!("desktop.{i}.log")),
                    directory.join(format!("desktop.{}.log", i + 1)),
                );
            }
            if fs::rename(&path, directory.join("desktop.1.log")).is_err() {
                return;
            }
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |t| t.as_secs());
            let _ = writeln!(file, "{stamp} {event}");
        }
    }
}
