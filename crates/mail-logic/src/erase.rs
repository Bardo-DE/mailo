//! What the person chose about deleting and erasing: what Delete does outside Trash and Spam, and
//! how long Trash and Spam keep a message. Which messages are due, and erasing them, are
//! `mail_core::erase`'s.

use chrono::TimeDelta;

/// What Delete does to mail outside Trash and Spam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delete {
    /// Moves it to Trash, where it waits for Trash's own time.
    #[default]
    ToTrash,
    /// Moves it to Trash, and erases it from there as soon as the server holds it.
    Now,
}

/// How long Trash or Spam keeps a message before it is erased.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Keep {
    /// Until it is erased by hand.
    #[default]
    Always,
    Day,
    Week,
    Month,
}

impl Keep {
    /// How long, when it ends.
    pub fn span(self) -> Option<TimeDelta> {
        match self {
            Keep::Always => None,
            Keep::Day => Some(TimeDelta::days(1)),
            Keep::Week => Some(TimeDelta::weeks(1)),
            Keep::Month => Some(TimeDelta::days(30)),
        }
    }
}

/// The person's choices about deleting and erasing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Policy {
    pub delete: Delete,
    pub trash: Keep,
    pub spam: Keep,
}

impl Policy {
    /// Whether there is nothing for a sweep to do: Delete moves to Trash, and both keep mail
    /// until it is erased by hand. The default.
    pub fn idle(&self) -> bool {
        *self == Policy::default()
    }
}
