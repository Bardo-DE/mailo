//! What adding an account is asked with and what it came to, as values: the servers a person
//! named, where the sign-in stands once the account is added, and what minting its Graph token
//! came to. Doing it, against the keyring and the store, is `mail_core::account`'s.

use mail_domain::presets;
use mail_domain::{HttpAuth, SaslMech};
use porter_provider::Issuer;

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
