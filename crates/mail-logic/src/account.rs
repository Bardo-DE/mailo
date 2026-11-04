//! What adding an account is asked with and what it came to, as values: the servers a person
//! named, where the sign-in stands once the account is added, and what minting its Graph token
//! came to, and why a sign-in or a removal ended without one. Doing it, against the keyring and the store, is `mail_core::account`'s.

use chrono::{DateTime, Utc};
use mail_domain::presets;
use mail_domain::{HttpAuth, SaslMech};
use porter_provider::Issuer;
use std::time::Duration;

/// Servers the user named, for an address the preset table does not cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setup {
    /// `--imap`: mail is read where it lies, in folders.
    Imap(presets::Manual),
    /// `--pop3`: mail is downloaded from one mailbox, and left there.
    Pop3(presets::ManualPop3),
    /// `--jmap [URL]`: a JMAP server, at the session URL given, or — with none — at
    /// `https://<domain>/.well-known/jmap`, found and confirmed before anything is sent there.
    Jmap {
        session: Option<String>,
        /// `--login NAME`, when the server wants something other than the whole address.
        login: Option<String>,
        /// Which header the secret travels in. The command line parses [`HttpAuth::Basic`]
        /// and `mail_core::account::add` makes it a bearer token when `MAILO_JMAP_TOKEN` is set;
        /// the window says which it was given.
        ///
        /// [`HttpAuth::Basic`]: HttpAuth::Basic
        auth: HttpAuth,
    },
    /// Found by discovery and confirmed by the user. Never parsed from arguments: the binary
    /// puts it here after asking, so `mail_core::account::add` can store it like any other.
    Discovered(Box<presets::Preset>),
}

/// Where the account's credential stands once it is added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The password, or a JMAP bearer token, is in the keyring.
    PasswordStored {
        /// It is a token, not a password.
        bearer: bool,
        /// The name the server is logged in with.
        login: String,
        /// Why the host will probably not take it, when the table knows.
        warning: Option<presets::PasswordWarning>,
    },
    /// No password was given; the account is configured and waits for one.
    PasswordMissing {
        /// A JMAP account, which can also be given a token.
        jmap: bool,
        login: String,
        sasl: Vec<SaslMech>,
    },
    /// The browser sign-in is done and its token is in the keyring.
    SignedIn {
        /// What minting a Graph token for sending came to, for an account that goes through Graph.
        graph: Option<GraphSetup>,
        client: ClientRecord,
    },
    /// The issuer's sign-in cannot start: this installation has no OAuth client id for it.
    NeedsClientId {
        issuer: Issuer,
        scopes: Vec<String>,
        /// The flags that make the same address find the same account again.
        route: MicrosoftRoute,
    },
}

/// What minting an account's Graph token came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphSetup {
    /// Reading and sending both go through Graph.
    ReadAndSend,
    /// Sending goes through Graph; reading does not.
    SendOnly,
    /// Graph would not issue a token. Mail is received; sending needs a permission consented to.
    Refused(String),
}

/// Whether the OAuth client of a sign-in was written down for renewing it later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientRecord {
    /// Nothing was recorded: the client was already known, or there is nowhere to write one.
    NotRecorded,
    /// The client id was written to this file.
    Recorded(std::path::PathBuf),
    /// It could not be written, so renewing the sign-in will need the client id given again.
    Failed(String),
}

/// How the address was added, as far as the Microsoft flags go. The address alone does not
/// reproduce a managed-tenant account, so a front-end that suggests the command to run again
/// needs these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrosoftRoute {
    /// Not a Microsoft 365 account named as such.
    Other,
    /// `--microsoft`: through Graph.
    Microsoft,
    /// `--microsoft --receive imap`.
    Imap,
    /// `--microsoft --send graph`.
    SendThroughGraph,
    /// `--microsoft --receive graph`.
    ReceiveThroughGraph,
}

/// How long the browser has to come back before the sign-in gives up. Long enough to find a
/// password or a phone for a second factor; short enough that a tab closed without a word does
/// not leave a sign-in waiting all day.
pub const WAIT: Duration = Duration::from_secs(10 * 60);

/// Whose sign-in it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// A new account: whichever the person signs in as.
    Add,
    /// An account already here whose sign-in has to be made again. The person must sign in as
    /// this address; its servers and how it reads and sends stay as they are.
    Again { address: String },
}

/// Why a direct sign-in ended without an account. Each says itself in plain words
/// (`Display`); what to offer next is the front end's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignInError {
    /// This installation has no OAuth client for the issuer, so its page cannot be opened.
    #[error("Mail has no sign-in set up for {}.", named(.0))]
    NoClient(Issuer),
    /// The issuer hosts no mail Mail reads, or the account does not sign in with it.
    #[error("Mail can't sign in to this account with {}.", named(.0))]
    NotMail(Issuer),
    /// The person closed or denied the sign-in on the issuer's page.
    #[error("Sign-in cancelled")]
    Declined,
    /// The sign-in was cancelled here, before the browser came back.
    #[error("Sign-in cancelled")]
    Cancelled,
    /// No browser came back in time.
    #[error("The sign-in took too long. Try again when you're ready.")]
    TimedOut,
    /// The person signed in again as someone other than the account that asked.
    #[error("You signed in as {signed}, not {asked}.")]
    OtherAccount { asked: String, signed: String },
    /// No browser would open the page.
    #[error("Couldn't open your browser: {0}")]
    Browser(String),
    /// The issuer could not be reached, or answered with something that was not a sign-in.
    #[error("Couldn't finish signing in: {0}")]
    Network(String),
    /// The account or its sign-in could not be kept.
    #[error("Couldn't save the account: {0}")]
    Store(String),
}

/// An issuer as the person knows it.
pub fn name_of(issuer: Issuer) -> &'static str {
    match issuer {
        Issuer::Google => "Google",
        Issuer::Microsoft => "Microsoft",
        Issuer::Fastmail => "Fastmail",
        Issuer::Dropbox => "Dropbox",
        Issuer::Box => "Box",
        _ => "this provider",
    }
}

/// [`name_of`], for the words of an error, which hold the issuer by reference.
fn named(issuer: &Issuer) -> &'static str {
    name_of(*issuer)
}

/// Whether accounts of `issuer` are mail Mail can add from a sign-in alone: the issuer names its
/// servers (`presets::preset_for_issuer`), so nothing is left to ask once the browser is done.
pub fn signs_in_directly(issuer: Issuer) -> bool {
    // Whether there is a preset at all: the time it would be stamped with does not matter.
    presets::preset_for_issuer(issuer, "", DateTime::<Utc>::default()).is_some()
}

/// Why an account was not removed. In every case the account and its mail are still there; after
/// [`RemoveError::Keyring`] or [`RemoveError::Store`], some of its saved sign-ins may already be
/// forgotten (see the module's words), and it asks to sign in again.
#[derive(Debug, thiserror::Error)]
pub enum RemoveError {
    /// No account has this id: removed already, or never added.
    #[error("no such account")]
    Unknown,
    /// The mail kept on this computer and nowhere else. It holds imported mail, which removing
    /// it would delete for good.
    #[error("the account that keeps mail on this computer cannot be removed")]
    Local,
    /// The keyring would not forget a sign-in: the account and its mail stay, and a sign-in
    /// forgotten before the refusal stays forgotten.
    #[error(
        "the keyring would not forget a saved sign-in, so the account was not removed \
         (it may ask to sign in again): {0}"
    )]
    Keyring(String),
    /// The database could not be read or written, after the sign-ins were forgotten: the account
    /// and its mail stay, and it asks to sign in again.
    #[error(
        "the database refused, so the account was not removed (it will ask to sign in again): {0}"
    )]
    Store(String),
}
