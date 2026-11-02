//! Tests of `mail-ui-list` that mount the whole window: they run over the window's fixtures
//! (`ui::fixtures`), which the list cannot have, as it cannot name `mail-app`'s shell or a store.

mod list;
mod row;
