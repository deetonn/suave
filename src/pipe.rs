use std::{
    io::{Error, ErrorKind},
    marker::PhantomData,
    path::Path,
};

// TODO: use lifetimes on LockFile and return a Lock<'lock> that is guaranteed to live
// the same length as the LockFile, so it can keep a reference to it, and close it when it is
// dropped.

use tokio::fs::{File, OpenOptions};

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
    pub fn new(parent: &'lock LockFile) -> Self {
        Self { parent }
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
    /// will be created if the lock does not yet exist.
    ///
    /// ## Example
    /// ```
    /// let lock = LockFile::connect("brand_new_resource")?;
    /// assert_eq!(lock.is_locked(), false);
    /// ```
    ///
    /// This basically acts as a mutex for whatever file you are trying
    /// to restrict to one writer at a time.
    pub async fn connect(name: impl AsRef<str>) -> Result<LockFile<'a>> {
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
        let is_locked = self.try_lock().await?;
        if is_locked {
            Ok(Lock::new(self))
        } else {
            Err(Error::new(ErrorKind::Other, "the file is already locked."))
        }
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
pub struct NamedPipe {
    _path: String,
    _lock: File,
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

impl NamedPipe {
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
    pub async fn connect(identifier: impl AsRef<str>) -> Result<NamedPipe> {
        todo!(
            "connect to a named pipe with identifier: `{}`",
            identifier.as_ref()
        )
    }

    /// Check if a pipe exists already.
    /// ```
    /// let exists = NamedPipe::exists("shared_resource");
    /// ```
    /// This only works if the lockfile exists aswell as the main shared resource.
    pub fn exists(identifier: impl AsRef<str>) -> bool {
        let unique_id = Self::generate_unique_id(identifier.as_ref());
        let actual_file = format!("{}{}.pipe", temporary_directory(), unique_id);
        let lock_file = format!("{}{}.lock", temporary_directory(), unique_id);

        if !Path::new(&lock_file).exists() {
            return false;
        }

        if !Path::new(&actual_file).exists() {
            return false;
        }

        true
    }

    /// Generate unique identifiers for any string. This is not random
    /// and just transforms the contents of the string.
    ///
    /// **NOTE**: This is public because file names are selected using this function.
    /// With this being public, anyone can understand how files are named, and therefore be able
    /// to debug the files.
    pub fn generate_unique_id(path: &str) -> String {
        let mut result = String::new();

        for ch in path.chars() {
            let ch = ch as u8;
            if ch > 90 {
                result.push((ch - 38) as char);
            } else {
                result.push((ch + 38) as char);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_create_lockfile() -> Result<()> {
        LockFile::connect("shared_resource").await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_lockfile_lock() -> Result<()> {
        let resource = LockFile::connect("shared_resource_2").await?;
        let lock = resource.lock().await?;

        eprintln!("locking and unlocking file: `{}`", resource._path);

        assert!(resource.is_locked().await?);
        drop(lock);
        assert!(!resource.is_locked().await?);

        Ok(())
    }

    #[tokio::test]
    async fn test_lockfile_multiple_opens() -> Result<()> {
        let resource = LockFile::connect("shared_resource_3").await?;
        let _lock = resource.lock().await?;

        assert!(resource.is_locked().await?);
        let faulty_lock = resource.lock().await;

        // should fail because its already locked above.
        assert!(faulty_lock.is_err());

        Ok(())
    }
}
