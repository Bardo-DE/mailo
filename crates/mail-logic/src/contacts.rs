//! What reading a `.vcf` file into the address book did, as a value. Reading it is
//! `mail_core::contacts`'s.

/// What reading a `.vcf` file did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Imported {
    /// Cards in the file.
    pub cards: usize,
    /// Addresses added to the book.
    pub addresses: usize,
    /// Cards of kind group, each now a group of this book.
    pub groups: usize,
    /// Cards of people, each now a card of this book.
    pub people: usize,
    /// Cards that are not groups and carried no email address: kept, but nothing to write to.
    pub empty: usize,
}
