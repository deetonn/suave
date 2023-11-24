# Suave :sunglasses:
Multi-process and interprocess communication library focused on getting things
up and working. It is 100% async and written in pure rust.

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
