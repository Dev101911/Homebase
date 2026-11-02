//! A per-server log file, rotated at 5 MB with 3 old files kept.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const KEEP: usize = 3;

pub struct LogFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max_bytes: u64,
}

impl LogFile {
    pub fn open(path: &Path) -> Self {
        Self::with_limit(path, MAX_BYTES)
    }

    pub fn with_limit(path: &Path, max_bytes: u64) -> Self {
        let mut log = LogFile {
            path: path.to_path_buf(),
            file: None,
            size: 0,
            max_bytes,
        };
        log.reopen();
        log
    }

    fn reopen(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
    }

    pub fn write_line(&mut self, line: &str) {
        if self.size >= self.max_bytes {
            self.rotate();
        }
        if let Some(f) = self.file.as_mut()
            && writeln!(f, "{line}").is_ok()
        {
            self.size += line.len() as u64 + 1;
        }
    }

    /// An agent event, marked so it stands out from claude's own output.
    pub fn event(&mut self, message: &str) {
        self.write_line(&format!("[homebase {}] {message}", timestamp()));
    }

    fn rotate(&mut self) {
        self.file = None;
        for i in (1..KEEP).rev() {
            let _ = std::fs::rename(numbered(&self.path, i), numbered(&self.path, i + 1));
        }
        let _ = std::fs::rename(&self.path, numbered(&self.path, 1));
        self.reopen();
    }
}

fn numbered(path: &Path, n: usize) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{n}"));
    PathBuf::from(s)
}

/// UTC, ISO 8601, without pulling in a date crate.
pub fn timestamp() -> String {
    let secs = crate::agent::now();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_759), (2026, 11, 2));
    }

    #[test]
    fn rotates_and_keeps_three() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.log");
        let mut log = LogFile::with_limit(&path, 10);
        for i in 0..10 {
            log.write_line(&format!("line number {i}"));
        }
        assert!(path.exists());
        assert!(numbered(&path, 3).exists());
        assert!(!numbered(&path, 4).exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "line number 9\n");
    }
}
