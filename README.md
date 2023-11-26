# Suave :sunglasses:
Multi-process and interprocess communication library focused on getting things
up and working. It is 100% async and written in pure rust.

Any and all methods should be supported and they are being added all the time. Come get involved.

# Features

## Easily create lockfiles

**NOTE**: This implementation requires no actual reading or writing.

```rs
use suave::pipe::LockFile;

let lockfile = LockFile::temp().await?;

{
  let lock = lockfile.lock().await?;
  // locked within this scope.
  eprintln!("Woohoo, its locked!");
} // unlocked on Drop.
```

## Use shared files to communicate

This implementation uses `LockFile` internally to sync writing.

```rs
use suave::pipe::NamedPipe;

// connect to this shared resource
let pipe = NamedPipe::connect("shared_resource").await?;
// aqquire the lock and write "Hello, World!"
let nbytes = pipe.write(b"Hello, World!").await?;
eprintln!("wrote {} bytes to our shared resource!", nbytes);
```

## Use the clipboard to communicate

This implementation uses `LockFile` internally to sync writing.

```rs
use suave::clipboard::Clipboard;

let clipboard = Clipboard::connect().await?;
let contents = clipboard.read()?;
eprintln!("initial contents: {}", contents);

clipboard.write("Something...", WriteKind::Guarantee).await?;
// Alternate way to just read clipboard contents, due to reading not needed a lock.
let new_contents = Clipboard::contents()?;
assert_eq!(new_contents, "Something...");
```
