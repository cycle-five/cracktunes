//! A placeholder reply -- `🔎 Searching...` -- has to be resolved on every way
//! out of the command that sent it (#494).
//!
//! The success path resolves it by editing it into the result. Every error
//! path used to leave it posted forever, because the placeholder was owned by
//! nothing: each `?` between the send and the edit returned with it still up.
//! The framework already reports a command's error as a message of its own
//! (`config.rs`, the `FrameworkError::Command` arm), so on failure the
//! placeholder is deleted rather than edited -- editing it would say the
//! error twice.

use crate::Context;
use poise::ReplyHandle;

/// Something a failed command can take back.
pub(crate) trait Discard {
    /// Remove the placeholder. Best effort: a failure here is logged and never
    /// allowed to replace the error that caused it.
    async fn discard(&self);
}

/// Pass `outcome` through unchanged, discarding `placeholder` first when it is
/// an error.
///
/// 🔑 Hand this the result of ONE future holding everything fallible between
/// sending the placeholder and turning it into the reply. A `?` inside that
/// future is covered without anyone having to list it; a `?` between the send
/// and this call is not, so keep the two adjacent. And end that future at the
/// successful edit: once the placeholder IS the reply, a later failure must
/// not delete it.
pub(crate) async fn discard_on_err<T, E>(
    placeholder: &impl Discard,
    outcome: Result<T, E>,
) -> Result<T, E> {
    if outcome.is_err() {
        placeholder.discard().await;
    }
    outcome
}

/// A sent reply, together with the context that can delete it.
pub(crate) struct Placeholder<'a> {
    pub(crate) ctx: Context<'a>,
    pub(crate) handle: &'a ReplyHandle<'a>,
}

impl Discard for Placeholder<'_> {
    async fn discard(&self) {
        if let Err(e) = self.handle.delete(self.ctx).await {
            tracing::warn!("could not remove a placeholder after its command failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A placeholder that only counts how often it was taken down.
    #[derive(Default)]
    struct Counted(AtomicUsize);

    impl Discard for Counted {
        async fn discard(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// #494's property, pinned where it lives rather than as a list of today's
    /// error sites: whatever failed, a failed outcome takes the placeholder
    /// down exactly once and hands the error on untouched.
    #[tokio::test]
    async fn a_failed_command_discards_its_placeholder_once_and_keeps_the_error() {
        let placeholder = Counted::default();

        let outcome: Result<(), &str> = discard_on_err(&placeholder, Err("resolver failed")).await;

        assert_eq!(outcome, Err("resolver failed"));
        assert_eq!(placeholder.0.load(Ordering::SeqCst), 1);
    }

    /// A successful one leaves it alone: by then it has been edited into the
    /// reply, and deleting it would delete the result.
    #[tokio::test]
    async fn a_successful_command_leaves_its_placeholder_alone() {
        let placeholder = Counted::default();

        let outcome: Result<u8, &str> = discard_on_err(&placeholder, Ok(3)).await;

        assert_eq!(outcome, Ok(3));
        assert_eq!(placeholder.0.load(Ordering::SeqCst), 0);
    }
}
