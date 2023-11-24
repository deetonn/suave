use std::{
    io::{Error, ErrorKind},
    path::Path,
};

// TODO: use lifetimes on LockFile and return a Lock<'lock> that is guaranteed to live
// the same length as the LockFile, so it can keep a reference to it, and close it when it is
// dropped.

use tokio::fs::File;

/// The result type used in this library. This just wraps `std::io::Error`.
pub type Result<T> = std::result::Result<T, std::io::Error>;

/// The exact size in bytes a file must be to have a state of
/// `LockState::Locked`.
pub const FLAG_LOCKED: usize = 77;
/// The exact size in bytes a file must be to have a state of
/// `LockState::Unlocked`
pub const FLAG_UNLOCKED: usize = 88;

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

/// Represents a lockfile. This file does not exclusively lock
/// any other file, it just provides mechanisms of knowing what
/// state it is in without actually reading the file.
pub struct LockFile {
    _path: String,
    _fh: tokio::fs::File,
}

impl LockFile {
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
    pub async fn connect(name: impl AsRef<str>) -> Result<LockFile> {
        let true_path = {
            let tmp_dir = temporary_directory();
            let id = NamedPipe::generate_unique_id(name.as_ref());
            format!("{}{}.lock", tmp_dir, id)
        };
        let path = Path::new(&true_path);
        if !path.exists() {
            let file_handle = tokio::fs::File::create(&path).await?;
            file_handle.set_len(FLAG_UNLOCKED as u64).await?;
            Ok(Self {
                _path: true_path,
                _fh: file_handle,
            })
        } else {
            let file_handle = tokio::fs::File::open(&path).await?;
            file_handle.set_len(FLAG_UNLOCKED as u64).await?;
            Ok(Self {
                _path: true_path,
                _fh: file_handle,
            })
        }
    }

    /// Get whether the lock is currently locked or not. This returns a result
    /// because if the file is deleted, reading the metadata would fail.
    ///
    /// This function just gets the metadata, and returns if the file size is equal to `FLAG_LOCKED`.
    pub async fn is_locked(&self) -> Result<bool> {
        let metadata = self._fh.metadata().await?;
        Ok(metadata.len() as usize == FLAG_LOCKED)
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
    ///
    /// **NOTE**: Better ways of doing this will come soon, such as a `Lock<'_>`
    /// being returned that will automatically unlock when dropped.
    ///
    /// This function returns `Ok(false)` when the file is already locked,
    /// and `Ok(true)` when the file has been locked and is now owned by you.
    ///
    /// Any error returned is to do with failure reading the files metadata.
    pub async fn try_lock(&self) -> Result<bool> {
        let metadata = self._fh.metadata().await?;
        if metadata.len() as usize == FLAG_LOCKED {
            return Ok(false);
        }
        self._fh.set_len(FLAG_UNLOCKED as u64).await?;
        Ok(true)
    }

    /// Attempt the unlock the file. Only call this function if you know that you
    /// own the current lock. (I.E **YOU** locked it.)
    ///
    /// This function returns `Err(...)` when we failure to read the metadata, or
    /// the file isn't locked to begin with.
    ///
    /// Otherwise, the file is marked as unlocked and `Ok(())` is returned.
    pub async fn unlock(&self) -> Result<()> {
        let metadata = self._fh.metadata().await?;
        if metadata.len() as usize != FLAG_LOCKED {
            return Err(Error::new(ErrorKind::Other, "the file is not locked."));
        }
        self._fh.set_len(FLAG_UNLOCKED as u64).await?;
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
        file.set_len(FLAG_UNLOCKED as u64).await?;

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

        if metadata.len() as usize == FLAG_LOCKED {
            return Err(Error::new(
                ErrorKind::WouldBlock,
                "the lockfile is currently locked.",
            ));
        }
        file.set_len(FLAG_LOCKED as u64).await?;

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
            if ch > 96 {
                result.push((ch - 32) as char);
            } else {
                result.push((ch + 32) as char);
            }
        }

        result
    }
}
