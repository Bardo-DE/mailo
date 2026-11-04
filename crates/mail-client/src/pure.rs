//! What a caller may name of `mail-core`: its words for mail (a document being edited, an undo, a
//! time as a person reads it) and what it does with no store and no server (parsing a search, the
//! files under the config directory). Everything that reads or writes the store, or asks a
//! server, is a call on [`Client`](crate::Client) instead, so a caller handed only a client has no
//! way to name a store.
//!
//! Each module stands for the `mail-core` module of the same name, and holds only what a caller
//! uses of it.

pub mod account {
    pub use mail_logic::account::{Mode, Outcome, RemoveError, Setup, SignInError};
    pub mod direct {
        pub use mail_logic::account::{WAIT, signs_in_directly};
        pub use mail_secrets::authorize::Profiles;
    }
    pub mod draft {
        pub use mail_logic::add_account::{
            Action, CopyMark, Draft, FieldDraft, ListKey, Out, Sheet, acted, is_manual, problem_of,
            retries, shown,
        };
    }
}
pub mod attach {
    use crate::CallError;
    use std::path::{Path, PathBuf};

    pub use mail_domain::human_size;
    pub use mail_logic::attach::{OpenedAttachment, runs_when_opened, safe_name};

    /// Where the window puts a file it has been asked to save: the folder the desktop names for
    /// downloads, `~/Downloads` when it names none, and the working directory as a last resort.
    pub fn downloads_dir() -> PathBuf {
        mail_core::attach::downloads_dir()
    }

    /// Attachment `index` of an opened message, numbered as the reader lists an opened message's
    /// attachments.
    pub fn opened_attachment(
        shown: &mail_mime::Parsed,
        index: usize,
    ) -> Result<OpenedAttachment, CallError> {
        Ok(mail_core::attach::opened_attachment(shown, index)?)
    }

    /// Save an opened message's attachment into `dir` under its name made safe, never over a file
    /// already there.
    pub fn save_opened(attachment: &OpenedAttachment, dir: &Path) -> Result<PathBuf, CallError> {
        Ok(mail_core::attach::save_opened(attachment, dir)?)
    }

    /// Write `bytes` into `dir` under `name` made safe, never over a file already there,
    /// returning the path written. How every file the window saves reaches the disk.
    pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, CallError> {
        Ok(mail_core::attach::write_new(dir, name, bytes)?)
    }
}
pub mod auth {
    pub use mail_logic::auth::{Standing, sentence, standing};
}
pub mod bimi {
    use mail_crypto::smime::Cert;
    use mail_mime::AuthResults;
    use mail_runtime::bimi::Txt;
    use std::path::Path;

    pub use mail_logic::bimi::{Setting, domain_of};
    pub use mail_runtime::bimi::{Cached, Lookup, Shared, cached, shared};

    /// What `bimi.json` in `dir` said, or off when there is none or it cannot be read. Never
    /// written.
    pub fn load(dir: &Path) -> Setting {
        mail_core::bimi::load(dir)
    }

    /// The mark verifying authorities' roots: the shipped ones and those in `dir`'s
    /// `bimi-roots.pem`. A file that does not read adds none.
    pub fn anchors(dir: &Path) -> Vec<Cert> {
        mail_core::bimi::anchors(dir)
    }

    /// The logo of the sender at `from`, as a PNG, when the setting is on, the believed `results`
    /// say DMARC passed for `from`'s domain, and a certified one is found (in the cache under
    /// `cache_dir`, or asked of `lookup`). Nothing is looked up unless the first two hold.
    pub async fn brand_logo<D: Txt + Sync>(
        setting: Setting,
        results: Option<&AuthResults>,
        from: &str,
        lookup: &Lookup<'_, D>,
        cache_dir: &Path,
    ) -> Option<Vec<u8>> {
        mail_core::bimi::brand_logo(setting, results, from, lookup, cache_dir).await
    }
}
pub mod compose {
    pub use mail_core::compose::SendError;
    pub use mail_logic::compose::doc::doc_of;
    pub use mail_logic::compose::weight::{Fit, Said, SendLimit, Weight, fit};
    pub use mail_logic::compose::{
        ATTACHMENT_BUDGET, Answer, Leaves, Myself, Quote, Redirected, Written, shows_inline,
    };
    pub use mail_logic::compose::addresses;
}
pub mod config {
    use crate::CallError;
    use std::path::{Path, PathBuf};

    /// Read `file_name` from `dir`. A missing file, or one that is not this type's JSON, is
    /// `T::default`: a damaged preference must not stop the window opening.
    pub fn read_json<T>(dir: &Path, file_name: &str) -> T
    where
        T: serde::de::DeserializeOwned + Default,
    {
        mail_core::config::read_json(dir, file_name)
    }

    /// Why `dir/file_name` cannot be read as a `T`: `None` when it is missing or reads. A file
    /// that is there and cannot be used is the person's, so a caller asks before putting the
    /// defaults in its place, and never writes over it.
    pub fn json_damage<T: serde::de::DeserializeOwned>(
        dir: &Path,
        file_name: &str,
    ) -> Option<String> {
        mail_core::config::json_damage::<T>(dir, file_name)
    }

    /// Write `value` as JSON to `dir/file_name`, creating `dir` if needed, by a temporary file
    /// and a rename, so a crash mid-write cannot leave a half-written file.
    pub fn write_json(
        dir: &Path,
        file_name: &str,
        value: &impl serde::Serialize,
    ) -> Result<(), CallError> {
        Ok(mail_core::config::write_json(dir, file_name, value)?)
    }

    /// Write `body` to `dir/file_name` by a temporary file and a rename, creating `dir` if
    /// needed.
    pub fn write_text(dir: &Path, file_name: &str, body: &str) -> Result<(), CallError> {
        Ok(mail_core::config::write_text(dir, file_name, body)?)
    }

    /// The user's home directory.
    pub fn home_dir() -> Option<PathBuf> {
        mail_core::config::home_dir()
    }

    /// mailo's config directory, in the system's per-user place; `None` when there is none.
    pub fn config_dir() -> Option<PathBuf> {
        mail_core::config::config_dir()
    }

    /// mailo's state directory: what was opened and expires, rather than a preference.
    pub fn state_dir() -> Option<PathBuf> {
        mail_core::config::state_dir()
    }

    /// mailo's cache directory: what can be fetched again.
    pub fn cache_dir() -> Option<PathBuf> {
        mail_core::config::cache_dir()
    }
}
pub mod contacts {
    pub use mail_logic::contacts::Imported;
    pub use mail_store::{CardId, Contact, Group, GroupHome, GroupId, Origin, PersonCard};
    pub mod groups {
        pub use mail_core::contacts::groups::{GroupError, Labelled, Offer, person_of};
    }
    pub mod people {
        pub use mail_core::contacts::people::{Added, PersonError, details};
        pub use mail_pim::vcard::Details;
    }
}
pub mod discover {
    pub use mail_logic::discover::{
        Failed, Found, Gap, Looked, Source, claimed_by, known, settle, typed, typed_domain,
    };
    pub mod providers {
        pub use mail_logic::discover::providers::set;
    }
}
pub mod export {
    pub use mail_core::export::{Exported, RawHead, Target, usual};
}
pub mod folder {
    pub use mail_domain::folder::Refusal;
}
pub mod formats {
    pub use mail_core::formats::{Writer, writer_named, writers};
}
pub mod import {
    pub use mail_core::import::{Destination, Imported, Source};
}
pub mod invite {
    use crate::CallError;
    use porter_calendar::{CalendarRef, CalendarStore, Event, Stored};

    pub use mail_domain::invite::Answered;
    pub use mail_logic::invite::{Followed, pick, repeats_words, say_when};

    /// Put `event` into `calendar`: a new event, or the one with its `UID` replaced.
    pub async fn add<S: CalendarStore>(
        store: &S,
        calendar: &str,
        event: &Event,
    ) -> Result<Stored, CallError> {
        Ok(mail_core::invite::add(store, calendar, event).await?)
    }

    /// Bring the calendar in step with an answer to the invitation `event` is: accepting or
    /// saying maybe puts it in, declining takes it out if it is there.
    pub async fn follow_answer<S: CalendarStore>(
        store: &S,
        calendar: &str,
        event: &Event,
        attendance: mail_domain::Attendance,
    ) -> Result<Followed, CallError> {
        Ok(mail_core::invite::follow_answer(store, calendar, event, attendance).await?)
    }

    /// Take the event a cancellation names out of `calendar`. `true` when it was there.
    pub async fn follow_cancellation<S: CalendarStore>(
        store: &S,
        calendar: &str,
        uid: &str,
    ) -> Result<bool, CallError> {
        Ok(mail_core::invite::follow_cancellation(store, calendar, uid).await?)
    }

    /// A calendar store's calendars, or none when it cannot say: a store that cannot be reached
    /// offers nothing to choose.
    pub async fn calendars<S: CalendarStore>(store: &S) -> Vec<CalendarRef> {
        mail_core::invite::calendars(store).await
    }
}
pub mod ipc {
    pub use mail_agent::ipc::agent;
    pub mod watching {
        pub use mail_agent::ipc::watching::running;
    }
}
pub mod mail {
    pub use mail_logic::clock::Clock;
}
pub mod message {
    pub use mail_core::message::{
        Bodies, FIRST_PAGE, Frames, INLINE_BYTES, Invited, Key, Looks, Offer, Opening, Protection,
        Receipt, Scheme, Sealed, Sent, Tried, ahead,
    };
}
pub mod notify {
    pub use mail_logic::notify::{Interest, Notification, Notifier, Places, Setting, Sound, Whom};

    /// What `notify.json` in `dir` said, or on when there is none or it cannot be read. Never
    /// written.
    pub fn load(dir: &std::path::Path) -> Setting {
        mail_core::notify::load(dir)
    }
    #[cfg(feature = "desktop")]
    pub mod desktop {
        /// What the app hands the macOS notification centre's banners through, and the click it
        /// hears back.
        #[cfg(target_os = "macos")]
        pub use mail_agent::notify::desktop::heard;
        pub use mail_agent::notify::desktop::{Desktop, Launch};
    }
    #[cfg(all(feature = "desktop", target_os = "macos"))]
    pub mod banner {
        pub use mail_agent::notify::banner::{Banner, Click, Posts, Pressed};
    }
}
pub mod offline {
    use crate::CallError;
    use std::path::Path;

    pub use mail_logic::offline::{Every, FILE_NAME, Keep, Kept, Remove, Window};

    /// The stored setting, or every account off when there is none or it cannot be read. Ask
    /// about a file that cannot be read ([`damage`]) before anything runs over it.
    pub fn load(dir: &Path) -> Kept {
        mail_core::offline::load(dir)
    }

    /// Why `offline.json` in `dir` cannot be read: `None` when it is missing or reads.
    pub fn damage(dir: &Path) -> Option<String> {
        mail_core::offline::damage(dir)
    }

    /// Set what `account` keeps, leaving every other account as it was. A file that cannot be
    /// read is left as it is, and nothing is set.
    pub fn save(dir: &Path, account: porter_core::AccountId, keep: Keep) -> Result<(), CallError> {
        Ok(mail_core::offline::save(dir, account, keep)?)
    }

    /// Write the setting in `dir` as `edit` leaves it, every other account as it was; a file that
    /// cannot be read is left as it is.
    pub fn change(dir: &Path, edit: impl FnOnce(Kept) -> Kept) -> Result<(), CallError> {
        Ok(mail_core::offline::change(dir, edit)?)
    }
}
pub mod pgp {
    pub use mail_core::pgp::{Ask, PgpError, Protected, WithSecret, no_passphrase};
    pub mod keys {
        pub use mail_core::pgp::keys::export_public;
    }
}
pub mod attachment_image {
    pub use mail_core::attachment_image::{
        Kind, LARGE, PDF_PAGE, Page, Picture, SMALL, Unavailable, pdf_page, picture, sniff,
    };
}
pub mod html_document {
    pub use mail_core::html_document::{LABELS, Pictures, Printed, write_file_into};
}
pub mod profile {
    pub use mail_logic::profile::{Profile, ProfileId};
}
pub mod provider {
    pub use mail_logic::provider::{Provider, provider, service_name};
    pub mod icon {
        pub use mail_core::provider::icon::{Loaded, missing, refresh};
    }
}
pub mod receipt {
    pub use mail_logic::receipt::ReceiptState;
}
pub mod remote_image {
    pub use mail_net::remote_image::{Image, ImageFetcher, Route};
}
pub mod rules {
    pub use mail_core::rules::ListedRule;
    pub mod manage {
        pub use mail_core::rules::manage::{Draft, SaveError, Sending, Step};
        pub use mail_domain::rule::DEFAULT_REPLY_DAYS;
        pub use mail_logic::rules::condition::{ConditionError, read_condition};
    }
    pub mod server {
        pub use mail_core::rules::server::{
            AwayServer, PushDone, away_server, instant, off_reaches_server, vacation_for,
        };
    }

    pub mod block {
        pub use mail_core::rules::block::Blocked;
    }
}
pub mod schedule {
    pub use mail_engine::schedule::{Daemon, Lost, Schedule, Timing, probe};
}
pub mod search {
    // `run` reads what it is handed: a caller has only the client's (`Client::source`).
    pub use mail_logic::search::{
        ActionHit, Affinity, Command, Find, Found, Highlight, MOST_FOUND, PersonHit, Results,
        SenderStats, Source, Step, Top, find_in, first, list_highlight, match_list, parse, run,
        search_list, snippet,
    };
}
pub mod server_search {
    pub use mail_logic::server_search::{Automatic, needs_server};

    /// What `server-search.json` in `dir` said, or off when there is none or it cannot be read.
    /// Never written.
    pub fn load(dir: &std::path::Path) -> Automatic {
        mail_core::server_search::load(dir)
    }
}
pub mod smime {
    pub use mail_core::smime::{Protected, SmimeError};
    pub mod certs {
        pub use mail_core::smime::certs::export;
    }
}
pub mod sync {
    pub use mail_logic::accounts::Configured;
    pub mod due {
        pub use mail_logic::accounts::IDLE;
    }
    pub mod fresh {
        pub use mail_logic::fetch::fresh::{Freshness, Pace, of_link};
    }
    pub mod report {
        pub use mail_logic::remedy::Offer;
    }
}
pub mod transfer {
    pub use mail_core::transfer::{IoCause, Refusal, Seen, expand_here, look, suggested};
}
pub mod unsubscribe {
    pub use mail_logic::unsubscribe::{Found, Outcome};

    /// The HTTP client a one-click unsubscribe is made with: HTTPS only, and no redirect off it.
    pub fn client() -> Result<reqwest::Client, crate::CallError> {
        mail_runtime::unsubscribe::client()
            .map_err(|e| crate::CallError::new(crate::Fault::Other, e.to_string()))
    }
}
pub mod conversation {
    pub use mail_logic::conversation::{
        Place, Say, Source, allowed_actions, default_places, find_place, folder_filter, folder_of,
        is_label_place, is_vips, kept_single, label_choices, mute_for_all, mute_op, offers, op_for,
        organises, own_places, place_key, places_with, reply_target, saved_of, saved_place, says,
        says_for_all, scoped, scoped_unread_filter, step, threading_of, turns_off, unread_filter,
    };
}
pub mod wanted {
    pub use mail_runtime::wanted::{Claim, Wanted};
}
pub use mail_proto::Storage;
pub mod junk {
    pub use mail_logic::junk::{Filtering, Heed, Order, Policy, Treat};
}
pub use mail_runtime::Logged;
pub use mail_runtime::graph::push::RelayRefused;
pub use mail_store::Syncing;
pub mod erase {
    pub use mail_logic::erase::{Delete, Keep, Policy};
}
pub mod signature {
    pub use mail_logic::signature::{Placement, nodes_of, purpose_of, recognise, swap};
}
pub mod vip {
    pub use mail_logic::vip::{Mark, Vips, toggled};
    pub use mail_store::Vip;
}
pub mod identity {
    pub use mail_domain::account::Typed;
}
