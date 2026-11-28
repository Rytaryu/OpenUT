//! Read-only filesystem input for the Hub log tailer.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use openut_hublog::tail::{LogFile, LogSource};

pub struct FsLogSource {
    root: PathBuf,
}

impl FsLogSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl LogSource for FsLogSource {
    type Error = io::Error;

    fn list(&self) -> io::Result<Vec<LogFile>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                // The Hub can remove or rotate a file while listing the folder.
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if metadata.is_file() {
                files.push(LogFile {
                    name,
                    len: metadata.len(),
                });
            }
        }
        Ok(files)
    }

    fn read_at(&self, name: &str, offset: u64, max: usize) -> io::Result<Option<Vec<u8>>> {
        if name.contains(['/', '\\', ':', '\0']) || Path::new(name).file_name().is_none() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "a log name must be a single filename",
            ));
        }
        let path = self.root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }

        // Keep the default Windows sharing flags. Close the handle after this read.
        let mut file = match OpenOptions::new().read(true).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(max as u64).read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use openut_hublog::decode::Decoder;

    use super::*;

    const SERVER: &str = "ViveTrackerServer_20261128.txt";
    const DRIVER: &str = "ViveTrackerDriver_20261128.txt";

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "openut-source-fs-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed),
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("could not make a test directory: {error}"),
                }
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn reads_a_utf16le_log_and_its_append_from_byte_offsets() -> io::Result<()> {
        let directory = TestDirectory::new();
        let source = FsLogSource::new(&directory.0);
        let path = directory.0.join(SERVER);
        let mut first = vec![0xff, 0xfe];
        first.extend(utf16le("# Log begin\r\n"));
        fs::write(&path, &first)?;

        let mut decoder = Decoder::new();
        let prefix = source.read_at(SERVER, 0, 3)?.expect("log exists");
        assert_eq!(prefix, first[..3]);
        assert!(decoder.push(&prefix).is_empty());
        let tail = source.read_at(SERVER, 3, first.len())?.expect("log exists");
        assert_eq!(tail, first[3..]);
        assert_eq!(decoder.push(&tail), ["# Log begin"]);

        let appended = utf16le("appended\r\n");
        OpenOptions::new()
            .append(true)
            .open(&path)?
            .write_all(&appended)?;
        let new_bytes = source
            .read_at(SERVER, first.len() as u64, appended.len())?
            .expect("log exists");
        assert_eq!(new_bytes, appended);
        assert_eq!(decoder.push(&new_bytes), ["appended"]);
        assert_eq!(source.list()?[0].len, (first.len() + appended.len()) as u64);
        let past_end = (first.len() + appended.len() + 1) as u64;
        assert_eq!(source.read_at(SERVER, past_end, 10)?, Some(vec![]));
        Ok(())
    }

    #[test]
    fn reads_a_log_with_the_read_only_attribute() -> io::Result<()> {
        let directory = TestDirectory::new();
        let source = FsLogSource::new(&directory.0);
        let path = directory.0.join(SERVER);
        let bytes = [0xff, 0xfe, b'#', 0];
        fs::write(&path, bytes)?;
        let initial = fs::metadata(&path)?.permissions();
        let mut read_only = initial.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only)?;
        let result = source.read_at(SERVER, 0, bytes.len());
        fs::set_permissions(&path, initial)?;
        assert_eq!(result?, Some(bytes.to_vec()));
        Ok(())
    }

    #[test]
    fn lists_regular_files_and_allows_disappearance() -> io::Result<()> {
        let directory = TestDirectory::new();
        let source = FsLogSource::new(&directory.0);
        let path = directory.0.join(SERVER);
        fs::write(&path, b"server")?;
        fs::create_dir(directory.0.join(DRIVER))?;
        let listed = source.list()?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, SERVER);
        assert_eq!(listed[0].len, 6);
        assert_eq!(source.read_at(DRIVER, 0, 6)?, None);

        fs::remove_file(path)?;
        assert_eq!(source.read_at(SERVER, 0, 6)?, None);
        assert!(source.list()?.is_empty());
        let missing_source = FsLogSource::new(directory.0.join("missing"));
        assert!(missing_source.list()?.is_empty());
        Ok(())
    }

    #[test]
    fn restricts_reads_to_direct_filenames() {
        let directory = TestDirectory::new();
        let source = FsLogSource::new(&directory.0);
        for invalid in [
            "",
            ".",
            "..",
            "../log",
            "folder/log",
            "folder\\log",
            "a:b",
            "a\0b",
        ] {
            let error = source
                .read_at(invalid, 0, 20)
                .expect_err("invalid log name");
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
    }
}
