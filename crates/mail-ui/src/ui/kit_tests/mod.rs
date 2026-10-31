//! Tests of `mail-ui-kit` that seed a store or mount the whole window: they run over the
//! window's fixtures (`ui::fixtures`), which the kit cannot have, as it cannot name `mail-app`'s
//! shell or a store.

mod data;
mod history;
mod host;
mod list_search;
mod live;
mod motion;
mod ops;
mod store_reads;
pub(in crate::ui) mod files_sheet_tests;
mod files_work_tests;
mod downloads_tests;
mod downloads_unedited_tests;
mod vip_tests;
mod move_to_tests;
mod menus_tests;
mod cache_tests;
pub(in crate::ui) mod pgp_keys_tests;
mod pgp_look_tests;
pub(in crate::ui) mod pgp_certs_tests;
mod pgp_smime_tests;
pub(in crate::ui) mod pgp_tests;
