use std::fs::{self, File, TryLockError};
use std::io;
use std::path::Path;

pub fn acquire(directory: Option<&Path>) -> io::Result<Option<File>> {
    let directory =
        directory.ok_or_else(|| io::Error::other("アプリのデータフォルダーが見つかりません。"))?;
    fs::create_dir_all(directory)?;
    // Keep the file in place so every launch locks the same file, even after a crash.
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("instance.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_launch_can_lock_only_after_the_first_releases_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = Some(directory.path());
        let first = acquire(path).unwrap().unwrap();
        assert!(acquire(path).unwrap().is_none());
        drop(first);
        assert!(acquire(path).unwrap().is_some());
    }
}
