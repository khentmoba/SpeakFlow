//! The local-only switch.
//!
//! When it is on, SpeakFlow must not reach the network. Every outbound feature
//! checks here before it builds a request, so the setting is enforced in one
//! place instead of being a promise in the interface.
//!
//! The flag is cached in a process-wide atomic because the checks sit on hot
//! paths. It is refreshed from disk at startup and whenever the setting changes.

use std::sync::atomic::{AtomicBool, Ordering};

static LOCAL_ONLY: AtomicBool = AtomicBool::new(false);

/// Loads the persisted value into the process-wide flag.
pub fn load() {
    LOCAL_ONLY.store(crate::utils::paths::read_local_only(), Ordering::SeqCst);
}

/// Applies an already-persisted value to the process-wide flag.
pub fn apply(enabled: bool) {
    LOCAL_ONLY.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    LOCAL_ONLY.load(Ordering::SeqCst)
}

/// Rejects an outbound action while local-only mode is on.
///
/// `what` names the action in the message the user sees, for example
/// "Download a model" or "Use cloud recognition".
pub fn block(what: &str) -> Result<(), String> {
    if is_enabled() {
        return Err(format!(
            "{what} needs a network connection, and Local-only mode is on. Turn it off in Settings → History & Privacy to allow it."
        ));
    }
    Ok(())
}

/// Convenience wrapper for call sites that return `Result<_, String>`.
pub fn ensure(what: &str) -> Result<(), String> {
    block(what)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag is process-wide, so these tests restore it before returning.
    struct Restore(bool);

    impl Drop for Restore {
        fn drop(&mut self) {
            LOCAL_ONLY.store(self.0, Ordering::SeqCst);
        }
    }

    #[test]
    fn blocking_only_applies_when_enabled() {
        let _restore = Restore(is_enabled());
        LOCAL_ONLY.store(false, Ordering::SeqCst);
        assert!(ensure("Download a model").is_ok());

        LOCAL_ONLY.store(true, Ordering::SeqCst);
        let error = ensure("Download a model").unwrap_err();
        assert!(error.contains("Download a model"));
        assert!(error.contains("Local-only mode"));
    }
}
