//! Reading what a decision is made over. Reads only: the stores and the settings files in, a
//! value out, so that what decides never reads either itself.

use crate::error::{CoreError, Logged};
use mail_logic::accounts::{Account, Accounts, Prefs, Settings};
use mail_logic::act::ThreadView;
use mail_store::{SqliteStore, Store};
use std::path::Path;

/// The settings the accounts are read with: the choices in `offline.json` under `config` (every
/// account at the defaults when there is no such directory or the file will not read) and the
/// junk policy the window set.
pub fn settings(config: Option<&Path>, junk: mail_logic::junk::Policy) -> Settings {
    Settings {
        kept: config.map(crate::offline::load).unwrap_or_default(),
        junk,
    }
}

/// Every account, in the order they were added, with its plan, what its server was last observed
/// to support and what `settings` say about syncing it.
///
/// An account whose plan or capabilities cannot be read is left out and logged, so that one bad
/// row does not take every other account's syncing with it. One nothing has connected for yet is
/// in, with no capabilities.
pub fn accounts(store: &SqliteStore, settings: &Settings) -> Result<Accounts, CoreError> {
    let mut out = Vec::new();
    for stored in store.list_accounts()? {
        match read_one(store, stored, settings) {
            Ok(account) => out.push(account),
            Err(error) => log::warn!("an account is left out: {error}"),
        }
    }
    Ok(Accounts::new(out))
}

/// One stored account as an [`Account`], or why it cannot be.
fn read_one(
    store: &SqliteStore,
    stored: mail_store::StoredAccount,
    settings: &Settings,
) -> Result<Account, CoreError> {
    let address = stored.address;
    let plan = stored.plan.map_err(|e| CoreError::PlanUnreadable {
        address: address.clone(),
        why: crate::sync::why(&e),
    })?;
    let caps = stored
        .caps
        .map(|caps| {
            caps.map_err(|e| CoreError::CapsUnreadable {
                address: address.clone(),
                why: crate::sync::why(&e),
            })
        })
        .transpose()?;
    let paused = store
        .account_prefs(stored.id.clone())
        .or_log("the account's preferences could not be read")
        .is_some_and(|prefs| prefs.syncing == mail_store::Syncing::Paused);
    Ok(Account {
        prefs: Prefs::of(settings, &stored.id),
        id: stored.id,
        address,
        plan,
        caps,
        paused,
    })
}

/// Every account as what an act is decided over: whether it has a server, and what that server was
/// last seen to support. What an act never reads (how often an account is synced, whether the
/// person paused it) is left as the defaults.
pub fn acting_accounts(store: &SqliteStore) -> Accounts {
    accounts(store, &Settings::default())
        .or_log("the accounts could not be read to act on mail")
        // Accounts that cannot be read are said above; with none known an act is made here alone.
        .unwrap_or_default()
}

/// The conversation `thread` and every message of it, for an act to be decided over. A message
/// that cannot be read is left out, and said: the operation then acts on the rest, as it would on a
/// conversation the message had left. `None` when the conversation cannot be read.
pub fn thread_view(store: &SqliteStore, thread: mail_domain::ThreadId) -> Option<ThreadView> {
    let loaded = store
        .thread(thread)
        .or_log("the conversation to act on could not be read")?;
    let messages = loaded
        .messages
        .iter()
        .filter_map(|id| {
            store
                .message(*id)
                .or_log("a message of the conversation could not be read")
        })
        .collect();
    Some(ThreadView {
        thread: loaded,
        messages,
    })
}

/// The label that marks junk on `account`, if it has one.
pub fn junk_label(
    store: &SqliteStore,
    account: &porter_core::AccountId,
) -> Option<mail_domain::LabelId> {
    store
        .labels(account.clone())
        .or_log("the labels could not be read to see whether one marks junk")?
        .into_iter()
        .find(|l| l.name.eq_ignore_ascii_case(mail_logic::junk::JUNK_LABEL))
        .map(|l| l.id)
}

/// What an arrival is decided over, read: the account's rules and labels, the VIPs, the user's
/// names, the conversations the messages are in and what is known of each message.
#[derive(Debug, Clone, Default)]
pub struct ArrivalInput {
    pub rules: Vec<mail_domain::Rule>,
    pub labels: Vec<mail_domain::Label>,
    pub vips: Vec<String>,
    pub names: Vec<String>,
    pub threads: Vec<ThreadView>,
    pub evidence: Vec<mail_logic::arrival::Evidence>,
}

impl ArrivalInput {
    /// The snapshot a decision is made over, for an account with or without a server and what its
    /// server was seen to support.
    pub fn snapshot<'a>(
        &'a self,
        account: &'a porter_core::AccountId,
        has_server: bool,
        caps: &'a mail_domain::AccountCaps,
        policy: mail_logic::junk::Policy,
    ) -> mail_logic::arrival::ArrivalSnapshot<'a> {
        mail_logic::arrival::ArrivalSnapshot {
            account,
            has_server,
            caps,
            rules: &self.rules,
            labels: &self.labels,
            vips: &self.vips,
            names: &self.names,
            policy,
            threads: &self.threads,
            evidence: &self.evidence,
        }
    }
}

/// Read what `messages` of `account` are decided over, with `rules` to run over them. A message
/// that is gone, or on another account, is left out.
///
/// What only the junk filter asks (the user's names, who the sender is, the server's verdict, the
/// counts of the words) is read for the messages it can judge, and not at all with the filter off.
pub fn arrival_input(
    store: &SqliteStore,
    account: &porter_core::AccountId,
    messages: &[mail_domain::MessageId],
    rules: Vec<mail_domain::Rule>,
    policy: &mail_logic::junk::Policy,
) -> Result<ArrivalInput, CoreError> {
    use mail_domain::{Body, MailboxRole};
    use mail_logic::arrival::{Evidence, Standing};
    use mail_logic::junk::{Filtering, Heed};
    let mut input = ArrivalInput {
        rules,
        labels: store.labels(account.clone())?,
        vips: store.vips()?.into_iter().map(|vip| vip.address).collect(),
        ..ArrivalInput::default()
    };
    let judging = policy.filtering == Filtering::On;
    if judging {
        input.names = store
            .identities(account.clone())?
            .into_iter()
            .filter_map(|identity| identity.from.name)
            .filter(|name| !name.trim().is_empty())
            .collect();
    }
    for id in messages {
        let message = match store.message(*id) {
            Ok(message) => message,
            Err(mail_store::StoreError::NoMessage(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        if message.account != *account {
            continue;
        }
        if !input
            .threads
            .iter()
            .any(|view| view.thread.summary.id == message.thread)
        {
            match thread_view(store, message.thread) {
                Some(view) => input.threads.push(view),
                None => continue,
            }
        }
        let mut standing = Standing::default();
        let judgeable = judging
            && message.mailbox == MailboxRole::Inbox
            && !matches!(message.body, Body::Absent);
        if judgeable {
            standing.acquaintance = acquaintance(store, &message.from.email);
            let spared =
                mail_logic::junk::spared(standing.acquaintance, &message, policy, &input.names);
            if !spared {
                if policy.server_says == Heed::Yes
                    && let Body::Present { raw, .. } = &message.body
                {
                    standing.server_spam = store
                        .blobs()
                        .get(*raw)
                        .is_ok_and(|bytes| mail_logic::junk::server_says_spam(&bytes));
                }
                if !standing.server_spam {
                    standing.counts =
                        store.junk_counts(account.clone(), &mail_logic::junk::words(&message))?;
                }
            }
        }
        input.evidence.push(Evidence {
            message: message.id,
            standing,
            places: store.placing(message.id)?,
        });
    }
    Ok(input)
}

/// What the records say of the sender at `address`: whether the contacts hold them, and whether
/// the user has written to them.
fn acquaintance(store: &SqliteStore, address: &str) -> mail_logic::junk::Acquaintance {
    let contact = store
        .contact(address)
        .or_log("a sender could not be looked up")
        .flatten();
    mail_logic::junk::Acquaintance {
        in_contacts: contact
            .as_ref()
            .is_some_and(|contact| contact.origin != mail_store::Origin::History),
        written_to: contact
            .as_ref()
            .is_some_and(|contact| contact.written.count > 0),
    }
}
