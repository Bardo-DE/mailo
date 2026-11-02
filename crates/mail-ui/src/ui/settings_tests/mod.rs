//! Tests of `mail-ui-settings` that mount the Settings window: they run over the window's
//! fixtures (`ui::fixtures`), which the settings cannot have, as it cannot name `mail-app`'s shell
//! or a store.

pub(in crate::ui) mod account;
pub(in crate::ui) mod contacts;
pub(in crate::ui) mod contacts_page;
pub(in crate::ui) mod default_mail;
pub(in crate::ui) mod offline;
pub(in crate::ui) mod push_relay;
pub(in crate::ui) mod rules_apply;
pub(in crate::ui) mod rules_away;
pub(in crate::ui) mod rules_page;
pub(in crate::ui) mod rules_work;
pub(in crate::ui) mod spaces;
pub(in crate::ui) mod syncing;
pub(in crate::ui) mod window;
