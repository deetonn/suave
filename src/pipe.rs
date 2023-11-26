use std::{
    future::Future,
    io::{Error, ErrorKind, Write},
    marker::PhantomData,
    path::Path,
};

// TODO: use lifetimes on LockFile and return a Lock<'lock> that is guaranteed to live
// the same length as the LockFile, so it can keep a reference to it, and close it when it is
// dropped.

use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncWrite, AsyncWriteExt},
    pin,
    time::sleep_until,
};

/// The result type used in this library. This just wraps `std::io::Error`.
pub type Result<T> = std::result::Result<T, std::io::Error>;

/// The exact size in bytes a file must be to have a state of
/// `LockState::Locked`.
pub const FLAG_LOCKED: u64 = 77;
/// The exact size in bytes a file must be to have a state of
/// `LockState::Unlocked`
pub const FLAG_UNLOCKED: u64 = 88;

pub(crate) async fn open_rw(path: impl AsRef<str>) -> Result<File> {
    let exists = Path::new(path.as_ref()).exists();
    OpenOptions::new()
        .write(true)
        .read(true)
        .create(!exists)
        .open(path.as_ref())
        .await
}

/// Which state a lockfile is in.
/// This information is gathered by reading the files size
/// and using specific flags
pub enum LockState {
    /// The lock is unlocked.
    Unlocked,
    /// The lock is locked.
    Locked,
    /// This is not a lockfile.
    NotALock,
}

impl LockState {
    /// initialize the `LockState` using a constant integer. This works
    /// with the defined constants `FLAG_LOCKED` and `FLAG_UNLOCKED`.
    pub const fn from_const(constant: u64) -> Self {
        match constant {
            FLAG_LOCKED => Self::Locked,
            FLAG_UNLOCKED => Self::Unlocked,
            _ => Self::NotALock,
        }
    }

    pub const fn locked(&self) -> bool {
        match self {
            Self::Locked => true,
            Self::Unlocked => false,
            Self::NotALock => false,
        }
    }
}

/// An instance of this represents a `LockFile` that is currently locked.
/// Once this goes out of scope, the `LockFile` is unlocked.
pub struct Lock<'lock> {
    parent: &'lock LockFile<'lock>,
}

impl<'lock> Lock<'lock> {
    /// Create the lock with the `LockFile` referenced. When
    /// this instance is dropped, `parent.unlock()` is called.
    ///
    /// **NOTE**: Shouldn't be called manually usually.
    pub async fn new(parent: &'lock LockFile<'lock>) -> Result<Lock<'lock>> {
        let is_ok = parent.try_lock().await?;
        if !is_ok {
            return Err(Error::new(ErrorKind::Other, "The file is already locked."));
        }
        Ok(Self { parent })
    }
}

/// This causes this object to automatically unlock the `LockFile` once it goes
/// out of scope.
impl<'lock> Drop for Lock<'lock> {
    fn drop(&mut self) {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.parent._path);

        if let Err(e) = file {
            panic!(
                "failed to open lockfile to unlock it in Lock::drop(): {} (file: {})",
                e, self.parent._path,
            );
        }

        let file = file.unwrap();
        let _ = file.sync_all();
        match file.set_len(FLAG_UNLOCKED) {
            Ok(_) => {}
            Err(e) => panic!("failed to unlock file by `file.set_len(...)`: {}", e),
        };
    }
}

/// Represents a lockfile. This file does not exclusively lock
/// any other file, it just provides mechanisms of knowing what
/// state it is in without actually reading the file.
pub struct LockFile<'a> {
    _path: String,
    _fh: tokio::fs::File,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> LockFile<'a> {
    /// Connect to the lock, storing information about it. This connection
    /// will be created if the lock does not yet exist. This lock will be located within the
    /// systems temporary directory.
    ///
    /// ### On Windows
    /// Path: C:\Users\UserName\AppData\Local\Temp\
    /// ### On Gnu/linux
    /// Path: /tmp/
    ///
    /// ## Example
    /// ```
    /// let lock = LockFile::connect("brand_new_resource").await?;
    /// assert_eq!(lock.state().locked(), false);
    /// ```
    ///
    /// This basically acts as a mutex for whatever file you are trying
    /// to restrict to one writer at a time.
    pub async fn temp(name: impl AsRef<str>) -> Result<LockFile<'a>> {
        let true_path = {
            let tmp_dir = temporary_directory();
            format!("{}{}.lock", tmp_dir, name.as_ref())
        };

        let file_handle = open_rw(&true_path).await?;
        file_handle.set_len(FLAG_UNLOCKED).await?;

        file_handle.sync_all().await?;

        Ok(Self {
            _path: true_path,
            _fh: file_handle,
            _phantom: PhantomData,
        })
    }

    /// Create the lock in the directory `path` under `name`.
    ///
    /// The usual `LockFile::connect()` return automatically creates the file in a temporary
    /// directory.
    /// ## Example
    pub async fn at(path: impl AsRef<Path>, name: impl AsRef<str>) -> Result<LockFile<'a>> {
        let path = format!("{}{}.lock", path.as_ref().display(), name.as_ref());

        let file_handle = open_rw(&path).await?;
        file_handle.sync_all().await?;

        Ok(Self {
            _path: path,
            _fh: file_handle,
            _phantom: PhantomData,
        })
    }

    /// Check if a lockfile with this same identifier already exists.
    /// ## Example
    /// ```
    /// if LockFile::exists("Pipeline") { /* ... */ } else { /* ... */ }
    /// ```
    ///
    /// **NOTE**: This isn't async because it's fairly simple.
    pub fn exists(identifier: impl AsRef<str>) -> bool {
        let path = format!("{}{}.lock", temporary_directory(), identifier.as_ref());
        Path::new(&path).exists()
    }

    /// Get whether the lock is currently locked or not. This returns a result
    /// because if the file is deleted, reading the metadata would fail.
    ///
    /// This function just gets the metadata, and returns if the file size is equal to `FLAG_LOCKED`.
    pub async fn is_locked(&self) -> Result<bool> {
        self._fh.sync_all().await?;
        let metadata = self._fh.metadata().await?;
        Ok(metadata.len() == FLAG_LOCKED)
    }

    /// The path to the lockfile. This will always return the path, but does not guarantee
    /// that the file still exists. To check this, just do:
    /// ```
    /// let lock = LockFile::new("resource")?;
    /// let path = Path::new(lock.path());
    /// if !path.exists() { ... } else { ... }
    /// ```
    pub fn path(&self) -> &String {
        &self._path
    }

    /// Attempt to lock the file, not allowing other processes to access it.
    ///
    /// ## Example
    /// ```
    /// let lock = LockFile::connect("shared_resource")?;
    /// let is_locked = lock.try_lock()?;
    ///
    /// if is_locked {
    ///     // ...
    /// }
    /// else {
    ///     // ...
    /// }
    /// ```
    /// This returns `Ok(false)` if the file is already locked.
    /// and `Ok(true)` when the file has been locked and is now owned by you.
    ///
    /// Any error returned is to do with failure reading the files metadata.
    pub async fn try_lock(&self) -> Result<bool> {
        let metadata = self._fh.metadata().await?;
        if metadata.len() == FLAG_LOCKED {
            return Ok(false);
        }
        self._fh.set_len(FLAG_LOCKED).await?;
        self._fh.sync_all().await?;
        Ok(true)
    }

    /// Lock the file, with the returned `Lock<'_>` being the locker.
    /// Whenever that instance is dropped, the lockfile is unlocked.
    ///
    /// # Example
    /// ```
    /// let resource = LockFile::connect("resource")?;
    /// let lock = resource.lock();
    /// // do stuff...
    /// drop(lock); // lockfile is now available.
    /// ```
    pub async fn lock(&'a self) -> Result<Lock<'a>> {
        self._fh.sync_all().await?;
        let lock = Lock::new(self).await?;
        Ok(lock)
    }

    /// Attempt the unlock the file. Only call this function if you know that you
    /// own the current lock. (I.E **YOU** locked it.)
    ///
    /// This function returns `Err(...)` when we failure to read the metadata, or
    /// the file isn't locked to begin with.
    ///
    /// Otherwise, the file is marked as unlocked and `Ok(())` is returned.
    pub async fn unlock(&self) -> Result<()> {
        self._fh.sync_all().await?;
        let metadata = self._fh.metadata().await?;
        if metadata.len() != FLAG_LOCKED {
            return Err(Error::new(ErrorKind::Other, "the file is not locked."));
        }
        self._fh.set_len(FLAG_UNLOCKED).await?;
        self._fh.sync_all().await?;
        Ok(())
    }

    /// Unlocks any file that is provided. This will make the file usable
    /// with `LockFile`.
    ///
    /// **WARNING**: This function uses `file.set_len()` and WILL destroy
    /// any contents within the file if its larger than the constant `FLAG_UNLOCKED`.
    ///
    /// This function *does not* check if the file is locked or not.
    /// It will always unlock to file and set the size to `FLAG_UNLOCKED`.
    ///
    /// ## Examples
    /// ### Bad example
    /// If there is important data in the file, do not use it as a lock.
    /// This example destroys the file provided.
    /// ```
    /// let random_file = "/usr/important.txt";
    /// LockFile::unlock_arbitrary_file(random_file)?;
    /// ```
    /// ### Good example
    /// This shows use using a special file we created for this purpose.
    /// Any instance of `LockFile` uses a `.lock` extension.
    /// ```
    /// let specific_for_this_case = "/tmp/lockfile.lock";
    /// LockFile::unlock_arbitrary_file(specific_for_this_case)?;
    /// ```
    pub async fn unlock_arbitrary_file(fully_qual_path: &str) -> Result<()> {
        let path = Path::new(fully_qual_path);

        if !path.exists() {
            return Err(Error::new(
                ErrorKind::NotFound,
                "the specified file to lock does not exist.",
            ));
        }

        let file = tokio::fs::File::open(fully_qual_path).await?;

        // Do no checking here.
        file.set_len(FLAG_UNLOCKED).await?;

        Ok(())
    }

    /// Retreive the underlying file instance.
    pub fn underlying_file(&'a self) -> &'a tokio::fs::File {
        &self._fh
    }

    /// Get what state the lock is currently in.
    /// ## Example
    /// ```
    /// let lock = LockFile::connect("resource").await?;
    /// if lock.state().locked() { /* wait? skip? */ }
    /// ```
    ///
    /// **NOTE**: If you get back a `LockState::NotALock`, something has externally
    /// modifiers the file and made it no longer valid. This is out of our control.
    pub async fn state(&self) -> Result<LockState> {
        self.underlying_file().sync_all().await?;
        let metadata = self.underlying_file().metadata().await?;
        Ok(LockState::from_const(metadata.len()))
    }

    /// Locks any file that is provided. This will make the file usable
    /// with `LockFile`.
    ///
    /// **WARNING**: This uses `file.set_len(...)` and could possibly
    /// truncate the contents of the file. Do not use this on files that
    /// contain important information, or any information at all.
    ///
    /// Please note that if the file is already locked (its size is
    /// equal to `FLAG_LOCKED`) this function will fail. Any spinlocking
    /// behaviour must take place within your own logic.
    /// ## Example
    /// ```
    /// let file = File::open("data.txt")?;
    /// // We can turn this file into a lock.
    /// LockFile::lock_arbitrary_file("data.txt")?;
    /// // Now we can easily use it.
    /// // NOTE: `LockFile::connect(...)` will call this automatically.
    /// ```
    /// We recomment using `LockFile::connect(...)`, however this exists for
    /// any niche situations that need it.
    pub async fn lock_arbitrary_file(fully_qual_path: &str) -> Result<()> {
        // To lock, we just write FLAG_LOCKED bytes to the file and call it a day.
        let path = Path::new(fully_qual_path);
        if !path.exists() {
            return Err(Error::new(
                ErrorKind::NotFound,
                "the specified file to lock does not exist.",
            ));
        }
        // We must also check that it ISNT locked yet.
        let file = tokio::fs::File::open(&path).await?;
        let metadata = file.metadata().await?;

        if metadata.len() == FLAG_LOCKED {
            return Err(Error::new(
                ErrorKind::WouldBlock,
                "the lockfile is currently locked.",
            ));
        }
        file.set_len(FLAG_LOCKED).await?;

        Ok(())
    }
}

/// A named pipe that different processes can use to communicate with this process.
/// done using a lockfile. We use the lockfiles size as a flag on which state its in.
///
/// The lockfile used is a `LockFile` with the same name as the pipe. They are seperated by
/// extension. Only one file can write to the shared resource at a time, and that is when they
/// have obtained a lock to the lock file.
pub struct NamedPipe<'a> {
    _pipe: File,
    _pipe_path: String,
    _lock: LockFile<'a>,
}

/// Get the temporary directory for this platform.
/// ## Example
/// ```
/// // on windows
/// let temp = temporary_directory();
/// assert_eq!(temp, "C:\\Users\\UserName\\AppData\\Local\\Temp\\");
///
/// // on gnu/linux
/// let temp = temporary_directory();
/// assert_eq!(temp, "/tmp/");
/// ```
/// **NOTE**: Any other platforms will cause a `compile_error!()` to occur.
pub fn temporary_directory() -> String {
    #[cfg(not(windows))]
    return String::from("/tmp/");

    #[cfg(windows)]
    {
        let user_name =
            std::env::var("USERPROFILE").expect("could not get user profile on windows.");
        return format!("{}\\AppData\\Local\\Temp\\");
    }

    #[cfg(not(windows))]
    #[cfg(not(not(windows)))]
    compile_error!("not sure how to compile the path for this platform.");
}

/// The result of a `write` function that was given a timeout.
pub enum WaitResult {
    /// The operation completed.
    Written { count: u64 },
    /// The operation did not complete and the timeout was exceeded.
    TimeoutHit,
}

/// alias of `tokio::time::Duration` and makes it easier to access.
pub type Duration = tokio::time::Duration;
/// alias of `std::time::Instant` and makes it easier to access.
pub type Instant = std::time::Instant;

impl<'a> NamedPipe<'a> {
    /// Attempt to connect to a named pipe. If the pipe does not exist, it will be
    /// created.
    /// ## Example
    /// ```
    /// let pipe = NamedPipe::connect("shared_resource").await?;
    /// assert_eq!(NamedPipe::exists("shared_resource"), true);
    /// ```
    /// ## Errors
    /// The only errors are if we fail to create or read information about
    /// the lockfile.
    pub async fn connect(identifier: impl AsRef<str>) -> Result<NamedPipe<'a>> {
        let lock = LockFile::temp(&identifier).await?;
        let full_path = format!("{}{}.pipe.v1", temporary_directory(), identifier.as_ref());

        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .create(true)
            .open(&full_path)
            .await?;

        Ok(Self {
            _pipe: file,
            _pipe_path: full_path,
            _lock: lock,
        })
    }

    /// Attempts to write all data into the pipe. This aqquires the lock.
    /// If the lock is already locked, an error is returned. Otherwise, it is aqquired
    /// and the data is written to the file.
    ///
    /// ## Example
    /// ```
    /// let pipe = NamedPipe::connect("shared").await?;
    /// let bytes_written = pipe.write(b"hello, world!").await?;
    /// ```
    pub async fn write(&mut self, data: &[u8]) -> Result<u64> {
        let state = self._lock.state().await?;
        if state.locked() {
            return Err(Error::new(
                ErrorKind::Other,
                "cannot write, the file is locked.",
            ));
        }
        // to write we must aqquire the lock.
        let _lock = self._lock.lock().await?;
        self._pipe.write_all(data).await?;

        Ok(data.len() as u64)
    }

    /// This function will attempt to claim the lock, however, if it does not succeed,
    /// it will wait for `timeout`. Once timeout is done, it will try again. If it fails the second
    /// time, `WaitResult::TimeoutHit` is returned. Otherwise, the action is executed. If all goes
    /// okay and no error occurs (will be returned in any `Err` variant) the variant
    /// `WaitResult::Written { count }` will be returned, with count as the amount of bytes
    /// written.
    ///
    /// ## Example
    /// ```
    /// use suave::pipe::{NamedPipe, Instant, Duration, WaitResult}
    ///
    /// let pipe = NamedPipe::connect("shared").await?;
    /// let result = pipe.write_waiting(b"hello, world!", Instant::now() +
    /// Duration::from_millis(100)).await?;
    ///
    /// let amount = match result {
    ///     WaitResult::Written { count } => count,
    ///     WaitResult::TimeoutHit => panic!("Oops! failed to aqquire lock.")
    /// };
    /// ```
    pub async fn write_timeout(&'a mut self, data: &[u8], timeout: Instant) -> Result<WaitResult> {
        if self._lock.state().await?.locked() {
            let _ = sleep_until(timeout.into()).await;
            if self._lock.state().await?.locked() {
                return Ok(WaitResult::TimeoutHit);
            }
        }

        // self.write(...) deals with the lock.
        let count = self.write(data).await?;

        Ok(WaitResult::Written { count })
    }

    /// Read as many bytes as the received buffer can fit into the buffer.
    /// This does not lock.
    ///
    /// ## Example:
    /// ```
    /// let pipe = NamedPipe::connect("shared").await?;
    /// let mut buffer: [u8; 128] = [0; 128];
    /// let amount_read = pipe.read_all(&mut buffer).await?;
    /// ```
    ///
    /// This does not guarantee all bytes to be written into the buffer, it an also not be
    /// guaranteed that this function will run async. Under the hood, this function uses
    /// `NamedPipe::read_to_string()` and just translates the data from this function
    /// into the buffer.
    ///
    /// **NOTE**: This function will never overflow the buffer, if the size of the buffer is
    /// smaller than that of the data in the resource, the buffer will be filled with what it can
    /// be.
    pub async fn read_all(&mut self, buffer: &mut [u8]) -> Result<u64> {
        let data = self.read_to_string().await?;
        let mut pos = 0;
        if data.len() > buffer.len() {
            // there is more data than there is buffer space, so we just write
            // what we can.
            while pos < buffer.len() {
                if let Some(ch) = data.chars().nth(pos) {
                    buffer[pos] = ch as u8;
                }
                pos += 1;
            }
        } else {
            // otherwise, we just copy all data from our string into the buffer.
            while pos < data.len() {
                if let Some(ch) = data.chars().nth(pos) {
                    buffer[pos] = ch as u8;
                }
            }
        }

        Ok(pos as u64)
    }

    /// The actual path to the file that is being used.
    ///
    /// # Example
    /// ```
    /// let pipe = NamedPipe::connect("shared").await?;
    /// assert!(pipe.path().display() == "/tmp/shared.pipe.v1");
    /// ```
    ///
    /// The above example is for linux machines, on windows the path would resole to
    /// `C:\Users\UserName\AppData\Local\Temp\shared.pipe.v1`
    pub fn path(&'a self) -> &'a Path {
        Path::new(&self._pipe_path)
    }

    /// Read the data into a vector. The vector will be expanded as needed.
    ///
    /// ## Example
    /// ```
    /// let pipe = NamedPipe::connect("shared").await?;
    /// let mut contents = Vec::new();
    /// let amount_read = pipe.read_buf(&mut contents).await?;
    /// ```
    ///
    /// This function has no guarantees to read the entire file or
    /// to even run async. Under the hood, this function uses `NamedPipe::read_to_string()`
    /// and just translates the data received from the function into the buffer.
    pub async fn read_buf(&mut self, buffer: &mut Vec<u8>) -> Result<u64> {
        let data = self.read_to_string().await?;
        for ch in data.chars() {
            buffer.push(ch as u8);
        }
        Ok(data.len() as u64)
    }

    /// Read the contents of the shared resource into a string.
    ///
    /// ## Example
    /// ```
    /// let resource = NamedPipe::connect("shared").await?;
    /// let data = resource.read_to_string().await?;
    ///
    /// eprintln!("data: {}", data); // outputs whatever was in the file.
    /// ```
    pub async fn read_to_string(&self) -> Result<String> {
        let path = Path::new(&self._pipe_path);
        if !path.exists() {
            return Err(Error::new(
                ErrorKind::NotFound,
                "named pipe has been deleted.",
            ));
        }
        tokio::fs::read_to_string(&path).await
    }
}

impl<'a> Into<LockFile<'a>> for NamedPipe<'a> {
    fn into(self) -> LockFile<'a> {
        self._lock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_create_lockfile() -> Result<()> {
        LockFile::temp("shared_resource").await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_lockfile_lock() -> Result<()> {
        let resource = LockFile::temp("shared_resource_2").await?;
        let lock = resource.lock().await?;

        assert!(resource.is_locked().await?);
        drop(lock);
        assert!(!resource.is_locked().await?);

        Ok(())
    }

    #[tokio::test]
    async fn test_lockfile_multiple_opens() -> Result<()> {
        let resource = LockFile::temp("shared_resource_3").await?;
        let _lock = resource.lock().await?;

        assert!(resource.is_locked().await?);
        let faulty_lock = resource.lock().await;

        // should fail because its already locked above.
        assert!(faulty_lock.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_shared_pipe_write() -> Result<()> {
        let data = b"hello, world!";
        let mut pipe = NamedPipe::connect("shared_resource").await?;

        let count = pipe.write(data).await?;
        assert!(count == 13, "count was {}", count);

        Ok(())
    }

    #[tokio::test]
    async fn test_shared_pipe_write_read() -> Result<()> {
        let data = b"other data!";
        let mut pipe = NamedPipe::connect("shared_rw").await?;

        let amount_written = pipe.write(data).await?;
        eprintln!("written {amount_written} bytes");
        let data = pipe.read_to_string().await?;

        assert!(data.len() == 11, "data length is invalid. {}", data.len());
        assert_eq!(data, "other data!");

        Ok(())
    }
}
