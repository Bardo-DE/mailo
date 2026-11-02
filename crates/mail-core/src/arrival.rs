//! Mail that has just arrived, acted on: the user's rules, muted conversations and the junk
//! filter, for every protocol alike.
//!
//! One small function per use-case: [`arrive`] reads what the decision is made over
//! ([`crate::gather::arrival_input`]), decides ([`mail_logic::arrival::on_arrival`]) and writes
//! the commit, all in one transaction so that a message that arrives meanwhile is not missed.
//! What a rule asks that only a running client can do comes back as the arrival's effects, for
//! [`crate::rules::speak::carry_out`] to carry out once the commit is written.
//!
//! [`apply_to`] and [`run_now`] are the same fold asked for by the user: over messages picked, or
//! over the mail already here.

use crate::error::CoreError;
use chrono::{DateTime, Utc};
use mail_domain::{AccountCaps, ChangeId, Filter, LabelId, MailboxRole, MessageId, Rule};
use mail_logic::arrival::{Arrival, ArrivalEffect, Fresh, Speaking};
use mail_logic::junk::Policy;
use mail_store::{SqliteStore, Store};
use porter_core::AccountId;

/// What an arrival came to, once written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Arrived {
    /// Messages a rule acted on, each with the names of the rules that did, in order.
    pub acted: Vec<(MessageId, Vec<String>)>,
    /// Messages a rule was run over.
    pub examined: usize,
    /// Messages that arrived in a muted conversation and were kept out of the inbox.
    pub muted: Vec<MessageId>,
    /// Messages the junk filter judged junk and moved or marked.
    pub junk: Vec<MessageId>,
    /// How many operations were queued for the server.
    pub queued: usize,
    /// What the rules asked that only a running client can do, for
    /// [`crate::rules::speak::carry_out`].
    pub effects: Vec<ArrivalEffect>,
}

/// Ids drawn at random, as the program wants them.
struct Generated;

impl Fresh for Generated {
    fn label(&mut self) -> LabelId {
        LabelId::generate()
    }
    fn change(&mut self) -> ChangeId {
        ChangeId::generate()
    }
}

/// Run the junk filter and the account's rules over what a pass stored for the first time on
/// `account`, as `policy` orders them, and keep a muted conversation's new mail out of the way.
///
/// `caps` is what the server was seen to support in the pass that brought the mail. The server's
/// half of every operation is queued with it, so what is queued leaves in the same pass.
pub fn arrive(
    store: &SqliteStore,
    account: &AccountId,
    has_server: bool,
    caps: &AccountCaps,
    arrived: &[MessageId],
    policy: Policy,
    now: DateTime<Utc>,
) -> Result<Arrived, CoreError> {
    if arrived.is_empty() {
        return Ok(Arrived::default());
    }
    store.within(|| {
        let rules = store.rules(account.clone())?;
        let input = crate::gather::arrival_input(store, account, arrived, rules, &policy)?;
        let snapshot = input.snapshot(account, has_server, caps, policy);
        let arrival = mail_logic::arrival::on_arrival(&snapshot, arrived, now, &mut Generated);
        write(store, arrival)
    })?
}

/// Run the account's enabled rules over messages the user picked, as if they had just arrived:
/// Mail's Apply Rules. Every action is carried out, those that send or notify included unless
/// `speaking` is [`Speaking::No`]. Messages on another account, or gone, are skipped.
pub fn apply_to(
    store: &SqliteStore,
    account: &AccountId,
    has_server: bool,
    caps: &AccountCaps,
    picked: &[MessageId],
    speaking: Speaking,
    now: DateTime<Utc>,
) -> Result<Arrived, CoreError> {
    store.within(|| {
        let rules = store.rules(account.clone())?;
        if rules.is_empty() {
            return Ok(Arrived::default());
        }
        let input =
            crate::gather::arrival_input(store, account, picked, rules, &Policy::default())?;
        let snapshot = input.snapshot(account, has_server, caps, Policy::default());
        let arrival =
            mail_logic::arrival::apply_rules(&snapshot, picked, speaking, now, &mut Generated);
        write(store, arrival)
    })?
}

/// Run `chosen` over the mail already on its account and within `within` (everything is
/// [`Filter::All`], a folder is [`Filter::InFolder`]), `batch` conversations at a time, calling
/// `progress` after each batch with what it did.
///
/// The rule runs whether or not it is enabled: being asked to is the point. Mail in Sent,
/// Drafts, Trash and Spam is left alone — the user's own mail, and mail already thrown away, is
/// not what "file what matches" means, and running a rule over Trash would bring it back out. Its
/// actions that send or notify are left out.
#[allow(clippy::too_many_arguments)]
pub fn run_now(
    store: &SqliteStore,
    has_server: bool,
    caps: &AccountCaps,
    chosen: &Rule,
    within: &Filter,
    batch: u32,
    now: DateTime<Utc>,
    progress: &mut dyn FnMut(&Arrived),
) -> Result<Arrived, CoreError> {
    use mail_domain::{AfterMatch, PageReq, Property, Query, RuleState, Sort, SortDir};
    let enabled = Rule {
        state: RuleState::Enabled,
        after: AfterMatch::Continue,
        ..chosen.clone()
    };
    let mut total = Arrived::default();
    let mut after = None;
    loop {
        let page = store.threads(
            &Query {
                filter: Filter::And(vec![
                    Filter::Account(chosen.account.clone()),
                    within.clone(),
                ]),
                sort: Sort {
                    property: Property::Date,
                    dir: SortDir::Desc,
                },
                page: PageReq {
                    after: after.clone(),
                    limit: batch.max(1),
                },
            },
            now,
        )?;
        let ran = store.within(|| {
            let mut picked = Vec::new();
            for summary in &page.items {
                for id in &store.thread(summary.id)?.messages {
                    let message = store.message(*id)?;
                    if !matches!(
                        message.mailbox,
                        MailboxRole::Sent
                            | MailboxRole::Drafts
                            | MailboxRole::Trash
                            | MailboxRole::Spam
                    ) {
                        picked.push(*id);
                    }
                }
            }
            let input = crate::gather::arrival_input(
                store,
                &chosen.account,
                &picked,
                vec![enabled.clone()],
                &Policy::default(),
            )?;
            let snapshot = input.snapshot(&chosen.account, has_server, caps, Policy::default());
            let arrival = mail_logic::arrival::apply_rules(
                &snapshot,
                &picked,
                Speaking::No,
                now,
                &mut Generated,
            );
            write(store, arrival)
        })??;
        progress(&ran);
        total.absorb(ran);
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(total),
        }
    }
}

/// Write the arrival's commit, if it holds anything.
fn write(store: &SqliteStore, arrival: Arrival) -> Result<Arrived, CoreError> {
    let queued = if arrival.writes() {
        store
            .commit(&arrival.commit)?
            .queued
            .iter()
            .flatten()
            .count()
    } else {
        0
    };
    Ok(Arrived {
        acted: arrival.acted,
        examined: arrival.examined,
        muted: arrival.muted,
        junk: arrival.junk,
        queued,
        effects: arrival.effects,
    })
}

impl Arrived {
    fn absorb(&mut self, other: Arrived) {
        self.acted.extend(other.acted);
        self.examined += other.examined;
        self.muted.extend(other.muted);
        self.junk.extend(other.junk);
        self.effects.extend(other.effects);
        self.queued += other.queued;
    }
}
