use std::io::{Error, ErrorKind};

use crate::pipe::{Duration, Instant, LockFile, Result};
/// The clipboard (global) that other processes can also read and write to.
use arboard::Clipboard as ArClipboard;

pub const CLIPBOARD_LOCKFILE: &str = "suave_clipboard";

/// This provides methods of using the clipboard to communicate,
/// however does use a lockfile to avoid bad writes.
///
/// ## Example
/// ```
/// let clipboard = Clipboard::connect().await?;
/// let initial_contents = clipboard.read()?;
/// eprintln!("initial clipboard contents: {}", initial_contents);
///
/// clipboard.write("Hello, World!", WriteKind::Guarantee).await?;
/// let contents = clipboard.read()?;
/// assert!(contents == "Hello, World!");
/// ```
pub struct Clipboard<'a> {
    lock: LockFile<'a>,
}

/// What kind of clipboard write is occuring.
pub enum WriteKind {
    /// Attempt to aqquire the lock and give up after one attempt,
    Immediate,
    /// Do not stop trying
    Guarantee,
    /// Give up after N attempts
    KeepTrying(usize),
    /// Give up after N attempts, with a wait of X between each attempt.
    KeepTryingTimeout(usize, Duration),
}

impl<'a> Clipboard<'a> {
    /// Connect to the clipboard lock. The instance returned provides a wrapper around
    /// `arboard::Clipboard` that is locked and allows each process to write one at a time.
    ///
    /// **NOTE**: If you want to connect without a lock at all, please use
    /// `ReadonlyClipboard`.
    ///
    /// ```
    /// let clipboard = Clipboard::connect().await?;
    /// eprintln!("we are now listening to the clipboard!");
    /// ```
    pub async fn connect() -> Result<Clipboard<'a>> {
        let lock = LockFile::temp(CLIPBOARD_LOCKFILE).await?;

        Ok(Self { lock })
    }

    /// Read whatever is currently inside of the clipboard.
    /// **NOTE**: This function does not lock and all processes can
    /// read at the same time without bother.
    ///
    /// # Example to get immediate contents
    /// ```
    /// let contents = Clipboard::connect().await?.read()?;
    /// eprintln!("Clipboard contents: {}", contents);
    /// ```
    ///
    /// This function is not async as it does not interact with the lockfile.
    pub fn read(&self) -> Result<String> {
        let mut cb = ArClipboard::new().map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        let response = cb.get().text();

        match response {
            Ok(contents) => Ok(contents),
            Err(e) => Err(Error::new(ErrorKind::Other, e.to_string())),
        }
    }

    /// Write `content` into the clipboard. The method used is dependent on `options`
    /// Please note that this does not append data by anymeans, it entirely overwrites whatever
    /// is currently in the clipboard.
    ///
    /// This method does not take `&mut self` as it doesn't modify struct state, but please be
    /// aware that this function *DOES* modify the clipboard contents.
    ///
    /// Supported clipboards are (as of UK date **26/11/2023**):
    ///   * Windows
    ///   * OsX
    ///   * Wayland
    ///   * X11
    ///
    /// ## Behaviour explainations
    /// * `WriteKind::Immediate` - Attempt to aqquire lock and write, give up after one attempt.
    /// * `WriteKind::Guarantee` - We will keep trying until the write succeeds.
    /// * `WriteKind::KeepTrying(N: usize)` - Will try `N` times, if `N` times is exceeded, this
    /// function fails.
    /// * `WriteKind::KeepTryingTimeout(N: usize, timeout: Instant)` - Will try `N` times, waiting `timeout` between each attempt, if `N` attempts are succeded with no luck, this function fails.
    ///
    /// ## Example
    /// *Attempt 3 times, waiting 200ms between attempts*
    /// ```
    /// let clipboard = Clipboard::connect().await?;
    /// if let Err(e) = clipboard.write("Hello, World!", WriteKind::KeepTryingTimeout(3,
    /// Instant::from_millis(200))).await {
    ///   eprintln!("failed after 3 attempts...");
    /// }
    /// else {
    ///   epritnln!("data was written successfully!");
    /// }
    /// ```
    ///
    /// **NOTE**: This is very unlikely to fail unless there are lots of processes writing to it,
    /// so unless there are 20 different processes attempting to write at once, `WriteKind::Guarantee` should be fine.
    pub async fn write(&self, content: impl Into<String>, options: WriteKind) -> Result<()> {
        let mut clipboard =
            ArClipboard::new().map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        match options {
            WriteKind::Immediate => {
                let _lock = self.lock.lock().await?;
                clipboard
                    .set_text(&content.into())
                    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
            }
            WriteKind::Guarantee => {
                let mut lock = self.lock.lock().await;
                while lock.is_err() {
                    lock = self.lock.lock().await;
                }
                clipboard
                    .set_text(&content.into())
                    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
            }
            WriteKind::KeepTrying(count) => {
                let mut ntimes = 0;
                let mut lock = self.lock.lock().await;
                while lock.is_err() {
                    ntimes += 1;
                    if ntimes >= count {
                        return Err(Error::new(ErrorKind::Other, "max retries reached."));
                    }
                    lock = self.lock.lock().await;
                }
                clipboard
                    .set_text(&content.into())
                    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
            }
            WriteKind::KeepTryingTimeout(count, timeout) => {
                let mut ntimes = 0;
                let mut lock = self.lock.lock().await;
                while lock.is_err() {
                    let timeout = tokio::time::Instant::now() + timeout;
                    let _ = tokio::time::sleep_until(timeout).await;
                    ntimes += 1;
                    if ntimes >= count {
                        return Err(Error::new(ErrorKind::Other, "max retries reached."));
                    }
                    lock = self.lock.lock().await;
                }
                clipboard
                    .set_text(&content.into())
                    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
            }
        }
    }

    /// Read the contents of the clipboard without creating an instance.
    /// Reading requires no lock, so this is quick and easy.
    ///
    /// ## Example
    /// ```
    /// let contents = Clipboard::contents()?;
    /// eprintln!("Hello, {}!", contents);
    /// ```
    pub fn contents() -> Result<String> {
        ArClipboard::new()
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?
            .get_text()
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    pub async fn test_clipboard_read_no_error() -> Result<()> {
        let clipboard = Clipboard::connect().await?;
        let content = clipboard.read()?;
        eprintln!("clipboard content: {}", content);
        Ok(())
    }

    // TODO: Add test cases for when other files are connected etc..

    #[tokio::test]
    pub async fn test_clipboard_write() -> Result<()> {
        let clipboard = Clipboard::connect().await?;
        clipboard
            .write("Hello, World!", WriteKind::Guarantee)
            .await?;

        let new_content = clipboard.read()?;
        assert!(new_content == "Hello, World!");
        Ok(())
    }
}
