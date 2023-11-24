<p style="text-align: center;">suave is a library for acheiving interprocess communication easily. It is 100% async.</p>

# Pipes

Easily create a lockfile using the `pipes` module.

## Example

```rs
use suave::pipes::LockFile;

let lockfile = LockFile::connect("unique_identifier").await?;

{
  let now_locked = lockfile.lock().await?;
  // Stuff now its locked...
}
// Now unlocked...
```
