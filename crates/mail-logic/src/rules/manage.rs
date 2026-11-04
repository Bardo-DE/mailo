//! A rule being written in an editor, which way a rule moves in the order and whether applying
//! rules may send mail. Keeping and running them is `mail_core::rules::manage`.

use super::ListedRule;
use mail_domain::{AfterMatch, RuleAction, RuleId};

/// A rule being written: what an editor's fields hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The rule being edited, or `None` for a new one.
    pub id: Option<RuleId>,
    pub name: String,
    /// The condition as typed.
    pub query: String,
    pub actions: Vec<RuleAction>,
    pub after: AfterMatch,
    /// A Reply answers one sender at most once in this many days.
    pub reply_days: u32,
}

impl Draft {
    /// A new rule, with nothing in it yet.
    pub fn blank() -> Self {
        Self {
            id: None,
            name: String::new(),
            query: String::new(),
            actions: Vec::new(),
            after: AfterMatch::Continue,
            reply_days: mail_domain::rule::DEFAULT_REPLY_DAYS,
        }
    }

    /// `listed`, opened for editing: its condition as the list shows it.
    pub fn of(listed: &ListedRule) -> Self {
        Self {
            id: Some(listed.rule.id),
            name: listed.rule.name.clone(),
            query: listed.condition.clone(),
            actions: listed.rule.actions.clone(),
            after: listed.rule.after,
            reply_days: listed.rule.reply_days,
        }
    }
}

/// Which way a rule moves in the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
}

/// Whether Apply Rules may send the mail its Reply and Forward rules ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sending {
    Yes,
    No,
}
