//! Storage for daily watcher summaries (watcher design §9.3).

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use openut_core::summary::Summary;
use openut_core::time::LogDate;
use serde::Deserialize;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const LOCK_NAME: &str = ".summary.lock";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteOutcome {
    Written,
    AlreadyFinal,
}

#[derive(Clone, Debug)]
pub struct SummaryWriter {
    directory: PathBuf,
}

impl SummaryWriter {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// Publish the latest summary without replacing an existing final summary.
    pub fn write(&self, summary: &Summary) -> io::Result<WriteOutcome> {
        validate_date(&summary.date)?;
        let _lock = self.lock()?;
        let path = self.directory.join(format!("{}.json", summary.date));
        if is_final_file(&path)? {
            return Ok(WriteOutcome::AlreadyFinal);
        }
        write_atomically(&self.directory, &path, summary)?;
        Ok(WriteOutcome::Written)
    }

    /// Keep the open summary before rebuilding, separate from the regular day file.
    pub fn save_before_rebuild(&self, summary: &Summary) -> io::Result<Option<PathBuf>> {
        if summary.is_final {
            return Ok(None);
        }
        validate_date(&summary.date)?;
        let _lock = self.lock()?;
        let prefix = format!("{}.before-rebuild-", summary.date);
        let mut latest = 0;
        for entry in fs::read_dir(&self.directory)? {
            let name = entry?.file_name();
            let number = name
                .to_str()
                .and_then(|name| name.strip_prefix(&prefix))
                .and_then(|suffix| suffix.strip_suffix(".json"))
                .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
                .and_then(|digits| digits.parse::<u64>().ok());
            latest = latest.max(number.unwrap_or_default());
        }
        let next = latest
            .checked_add(1)
            .ok_or_else(|| io::Error::other("summary backup number exhausted"))?;
        let path = self.directory.join(format!("{prefix}{next}.json"));
        write_atomically(&self.directory, &path, summary)?;
        Ok(Some(path))
    }

    fn lock(&self) -> io::Result<File> {
        fs::create_dir_all(&self.directory)?;
        // Keep the lock file in place so every writer locks the same file, even
        // as the daily JSON files are replaced.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(LOCK_NAME))?;
        file.lock()?;
        Ok(file)
    }
}

fn validate_date(date: &str) -> io::Result<()> {
    fn parse(date: &str) -> Option<LogDate> {
        if date.len() != 10 || !date.is_ascii() || &date[4..5] != "-" || &date[7..8] != "-" {
            return None;
        }
        let [year, month, day] = [&date[..4], &date[5..7], &date[8..]];
        if ![year, month, day]
            .into_iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return None;
        }
        let date = LogDate {
            year: year.parse().ok()?,
            month: month.parse().ok()?,
            day: day.parse().ok()?,
        };
        date.days_since_epoch()?;
        Some(date)
    }

    parse(date).map(|_| ()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a YYYY-MM-DD summary date",
        )
    })
}

fn is_final_file(path: &Path) -> io::Result<bool> {
    #[derive(Deserialize)]
    struct SummaryStatus {
        #[serde(rename = "final")]
        is_final: bool,
    }

    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    // Preserve files whose status cannot be read, since they may be final.
    serde_json::from_reader::<_, SummaryStatus>(BufReader::new(file))
        .map(|status| status.is_final)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "could not read saved summary status",
            )
        })
}

fn write_atomically(directory: &Path, path: &Path, summary: &Summary) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(summary).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let (mut temporary, mut file) = TemporaryFile::create(directory)?;
    let written = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file);
    written?;
    fs::rename(&temporary.path, path)?;
    temporary.published = true;
    Ok(())
}

struct TemporaryFile {
    path: PathBuf,
    published: bool,
}

impl TemporaryFile {
    fn create(directory: &Path) -> io::Result<(Self, File)> {
        loop {
            let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(".summary-{}-{number}.tmp", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok((
                        Self {
                            path,
                            published: false,
                        },
                        file,
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            loop {
                let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
                let name = format!("openut-summary-test-{}-{number}", std::process::id());
                let path = std::env::temp_dir().join(name);
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("could not create summary test folder: {error}"),
                }
            }
        }

        fn entries(&self) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .filter(|name| name != LOCK_NAME)
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample(date: &str, is_final: bool, unobserved_s: f64) -> Summary {
        Summary {
            schema: 1,
            date: date.to_owned(),
            is_final,
            observed_until: format!("{date} 00:00:20.000"),
            unobserved_s,
            source: Default::default(),
            trackers: Vec::new(),
            host_starts: 0,
            faults: Default::default(),
            detections: Default::default(),
            pushes: 0,
        }
    }

    fn read_json(path: &Path) -> serde_json::Value {
        serde_json::from_reader(BufReader::new(File::open(path).unwrap())).unwrap()
    }

    #[test]
    fn publishes_open_updates_and_preserves_the_final_file_after_restart() {
        let directory = TestDirectory::new();
        let date = "2026-11-28";
        let path = directory.0.join("2026-11-28.json");
        let writer = SummaryWriter::new(&directory.0);
        for (is_final, unobserved) in [(false, 10.0), (false, 20.0), (true, 25.0)] {
            assert_eq!(
                writer.write(&sample(date, is_final, unobserved)).unwrap(),
                WriteOutcome::Written
            );
            let value = read_json(&path);
            assert_eq!(value["date"], date);
            assert_eq!(value["final"], is_final);
            assert_eq!(value["unobserved_s"], unobserved);
        }
        let final_bytes = fs::read(&path).unwrap();
        let restarted = SummaryWriter::new(&directory.0);
        for is_final in [false, true] {
            assert_eq!(
                restarted.write(&sample(date, is_final, 99.0)).unwrap(),
                WriteOutcome::AlreadyFinal
            );
        }
        assert_eq!(fs::read(path).unwrap(), final_bytes);
        assert_eq!(directory.entries(), ["2026-11-28.json"]);
    }

    #[test]
    fn keeps_numbered_rebuild_backups_separate_from_the_current_day_file() {
        let directory = TestDirectory::new();
        let writer = SummaryWriter::new(&directory.0);
        let day = sample("2026-11-28", false, 10.0);
        writer.write(&day).unwrap();
        let first = writer.save_before_rebuild(&day).unwrap().unwrap();
        let reopened = SummaryWriter::new(&directory.0);
        let rewritten = sample("2026-11-28", false, 20.0);
        let second = reopened.save_before_rebuild(&rewritten).unwrap().unwrap();
        let other_day = reopened
            .save_before_rebuild(&sample("2026-11-27", false, 30.0))
            .unwrap()
            .unwrap();
        assert_eq!(
            first.file_name().unwrap(),
            "2026-11-28.before-rebuild-1.json"
        );
        assert_eq!(
            second.file_name().unwrap(),
            "2026-11-28.before-rebuild-2.json"
        );
        assert_eq!(
            other_day.file_name().unwrap(),
            "2026-11-27.before-rebuild-1.json"
        );
        assert_eq!(read_json(&first)["unobserved_s"], 10.0);
        assert_eq!(read_json(&second)["unobserved_s"], 20.0);
        assert_eq!(
            read_json(&directory.0.join("2026-11-28.json"))["unobserved_s"],
            10.0
        );
        assert!(
            reopened
                .save_before_rebuild(&sample("2026-11-28", true, 99.0))
                .unwrap()
                .is_none()
        );
        assert_eq!(directory.entries().len(), 4);
    }

    #[test]
    fn concurrent_open_and_final_writers_keep_the_final_version() {
        let directory = TestDirectory::new();
        let writer = SummaryWriter::new(&directory.0);
        let ready = Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                ready.wait();
                writer.write(&sample("2026-11-28", true, 30.0)).unwrap();
            });
            scope.spawn(|| {
                ready.wait();
                writer.write(&sample("2026-11-28", false, 20.0)).unwrap();
            });
        });
        let saved = read_json(&directory.0.join("2026-11-28.json"));
        assert_eq!(saved["final"], true);
        assert_eq!(saved["unobserved_s"], 30.0);
        assert_eq!(directory.entries(), ["2026-11-28.json"]);
    }

    #[test]
    fn does_not_replace_a_summary_whose_final_flag_cannot_be_read() {
        let directory = TestDirectory::new();
        let writer = SummaryWriter::new(&directory.0);
        let path = directory.0.join("2026-11-28.json");
        fs::write(&path, b"{\"final\":").unwrap();
        let error = writer
            .write(&sample("2026-11-28", false, 10.0))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(path).unwrap(), b"{\"final\":");
        assert_eq!(directory.entries(), ["2026-11-28.json"]);
    }

    #[test]
    fn removes_the_temporary_file_when_promotion_fails() {
        let directory = TestDirectory::new();
        let path = directory.0.join("2026-11-28.json");
        fs::create_dir(&path).unwrap();
        assert!(write_atomically(&directory.0, &path, &sample("2026-11-28", false, 10.0)).is_err());
        assert_eq!(directory.entries(), ["2026-11-28.json"]);
        assert!(path.is_dir());
    }

    #[test]
    fn invalid_dates_do_not_create_files_or_folders() {
        let directory = TestDirectory::new();
        let writer = SummaryWriter::new(directory.0.join("summary"));
        for date in ["../2026-11-28", "2026-11-31", "2026-1-02", "2026-11-2é"] {
            let invalid = sample(date, false, 0.0);
            assert_eq!(
                writer.write(&invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(
                writer.save_before_rebuild(&invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert!(directory.entries().is_empty());
    }
}
