use std::fs::{File, TryLockError};

/// Explicitly release ownership even if a fork temporarily inherited the file
/// description. Store this after the protected handles so they close first.
pub(crate) struct WriterLock(pub(crate) File);

impl WriterLock {
    pub(crate) fn acquire(file: File) -> Result<Self, TryLockError> {
        file.try_lock()?;
        Ok(Self(file))
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
