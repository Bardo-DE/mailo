//! Why a call on the client did not do what it was asked: one error for every call, in words a
//! front end can show, with the few kinds a front end answers differently told apart.
//!
//! The crates below the client each have their own failures (a store's, the core's, a protocol's);
//! a caller of the client names none of them. It reads the words ([`CallError`]'s `Display`), asks
//! which step mends it ([`CallError::remedy`]) and, for the handful of failures it has something
//! of its own to say about, matches [`CallError::fault`].

use mail_core::{CoreError, RelayRefused};
use mail_domain::DraftId;
use mail_logic::remedy::Remedy;
use mail_store::StoreError;

/// The failures a caller answers in its own way. Every other failure is [`Fault::Other`]: said in
/// its own words, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The file is not a picture a message body can show.
    NotAnInlineImage,
    /// The pictures in the body would come to more than a message may carry.
    InlineImagesTooBig,
    /// The draft is on its way out; what goes out was frozen when it was sent.
    DraftOnItsWay,
    /// The incoming server was reached and would not take the sign-in.
    SignInRefused,
    /// The incoming server could not be reached to try the sign-in.
    ServerUnreachable,
    /// The push relay's address cannot be one.
    Relay(RelayRefused),
    /// The attachment was left on the server and is fetched when opened.
    AttachmentOnServer,
    /// The message has no attachment of that number.
    NoAttachment,
    /// There is no such draft (any more).
    NoDraft,
    /// There is no such message (any more).
    NoMessage,
    /// Anything else.
    Other,
}

/// A call on the client that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    fault: Fault,
    said: String,
    remedy: Option<Remedy>,
}

impl CallError {
    /// A failure of kind `fault`, in `said`.
    pub fn new(fault: Fault, said: impl Into<String>) -> Self {
        CallError {
            fault,
            said: said.into(),
            remedy: None,
        }
    }

    /// The same failure, with the step that mends it.
    pub fn mended_by(mut self, remedy: Remedy) -> Self {
        self.remedy = Some(remedy);
        self
    }

    /// There is no draft `id`.
    pub fn no_draft(id: DraftId) -> Self {
        CallError::new(Fault::NoDraft, format!("no such draft: {id}"))
    }

    /// What kind of failure this is.
    pub fn fault(&self) -> &Fault {
        &self.fault
    }

    /// The step that mends it, when there is one: the front end words it.
    pub fn remedy(&self) -> Option<&Remedy> {
        self.remedy.as_ref()
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.said)
    }
}

impl std::error::Error for CallError {}

impl From<CallError> for String {
    fn from(error: CallError) -> Self {
        error.said
    }
}

impl From<CoreError> for CallError {
    fn from(error: CoreError) -> Self {
        let fault = match &error {
            CoreError::NotAnInlineImage { .. } => Fault::NotAnInlineImage,
            CoreError::InlineImagesTooBig { .. } => Fault::InlineImagesTooBig,
            CoreError::DraftOnItsWay => Fault::DraftOnItsWay,
            CoreError::SignInRefused { .. } => Fault::SignInRefused,
            CoreError::ServerUnreachable { .. } => Fault::ServerUnreachable,
            CoreError::Relay(refused) => Fault::Relay(refused.clone()),
            CoreError::AttachmentOnServer { .. } => Fault::AttachmentOnServer,
            CoreError::NoAttachment { .. } => Fault::NoAttachment,
            CoreError::Store(StoreError::NoDraft(_)) => Fault::NoDraft,
            CoreError::Store(StoreError::NoMessage(_)) => Fault::NoMessage,
            _ => Fault::Other,
        };
        CallError {
            fault,
            said: error.to_string(),
            remedy: error.remedy(),
        }
    }
}

/// What the lower crates' failures say, through [`CoreError`], which wraps each of them.
macro_rules! through_core {
    ($($lower:ty),+) => {$(
        impl From<$lower> for CallError {
            fn from(error: $lower) -> Self {
                CoreError::from(error).into()
            }
        }
    )+};
}

through_core!(
    mail_core::compose::SendError,
    mail_core::pgp::PgpError,
    mail_core::smime::SmimeError
);

impl From<StoreError> for CallError {
    fn from(error: StoreError) -> Self {
        let fault = match &error {
            StoreError::NoDraft(_) => Fault::NoDraft,
            StoreError::NoMessage(_) => Fault::NoMessage,
            _ => Fault::Other,
        };
        CallError {
            fault,
            said: error.to_string(),
            remedy: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failure core can mend carries the step along; one that has none carries none.
    #[test]
    fn a_core_failure_keeps_its_words_and_its_step() {
        for failure in [
            CoreError::NoAccounts,
            CoreError::HeadersOnly,
            CoreError::NotDownloaded,
            CoreError::PublishedEvent,
            CoreError::DaemonRunning,
            CoreError::NoIdentityToSendAs,
        ] {
            let words = failure.to_string();
            let call = CallError::from(failure);
            assert_eq!(call.to_string(), words);
            assert!(call.remedy().is_some(), "no step for {words}");
        }
        let plain = CallError::from(CoreError::NoMessageId);
        assert_eq!(plain.remedy(), None);
        assert_eq!(plain.fault(), &Fault::Other);
    }

    /// The failures a front end answers in its own way are told apart.
    #[test]
    fn the_failures_a_caller_answers_are_told_apart() {
        let call = |failure| CallError::from(failure).fault().clone();
        assert_eq!(call(CoreError::DraftOnItsWay), Fault::DraftOnItsWay);
        assert_eq!(
            call(CoreError::NotAnInlineImage { name: "a".into() }),
            Fault::NotAnInlineImage
        );
        assert_eq!(
            call(CoreError::Relay(RelayRefused::NotHttps)),
            Fault::Relay(RelayRefused::NotHttps)
        );
        assert_eq!(
            CallError::no_draft(DraftId::generate()).fault(),
            &Fault::NoDraft
        );
    }
}
