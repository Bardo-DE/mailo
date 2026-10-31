//! Tests of `mail-ui-reader` that seed a store or mount the whole window: they run over the
//! window's fixtures (`ui::fixtures`), which the reader cannot have.

pub(in crate::ui) mod invite_card_tests;
mod invite_tests;
mod print_pdf_tests;
mod print_tests;
mod reading_fetch_tests;
mod reading_tests;
mod receipt_tests;
mod sender_tests;
mod unsubscribe_tests;
mod reading_header_inline_tests;
mod reading_peek_part_inline_tests;
