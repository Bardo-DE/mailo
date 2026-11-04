//! VIPs: the people whose mail matters most, as Mail on the Mac has them.
//!
//! A mark on a person, kept by the store ([`mail_store::Store::put_vip`]) by the address the book
//! keys them by, and one list across every account and Space. A Space's pinned people are that
//! Space's shortcuts and stay so; they are offered as VIPs to mark, not made VIPs by being pinned.
//!
//! What a VIP changes is said here once, for every surface: `is:vip` finds the conversations any
//! of them wrote in ([`Filter::Vip`], which asks the store's list whenever it is run, so a saved
//! view or a rule follows the list), the VIPs place lists them, a row and the reader mark their
//! name, and "Notify for: VIPs only" announces their mail alone
//! ([`crate::notify::Whom`]).

use crate::error::{CoreError, Logged};
use mail_store::Store;

pub use mail_store::Vip;

pub use mail_logic::vip::{Mark, Vips, toggled};

/// The VIPs the store holds, or none when it cannot say: a list that cannot be read marks
/// nobody, and is logged.
pub fn read<S: Store + ?Sized>(store: &S) -> Vips {
    Vips::new(
        store
            .vips()
            .or_log_default("the VIPs could not be read")
            .into_iter()
            .map(|vip| vip.address),
    )
}

/// Every VIP, with the name to list each by: the book's, else the one they were marked with,
/// else none.
pub fn listed(store: &dyn Store) -> Result<Vec<Vip>, CoreError> {
    Ok(store
        .vips()?
        .into_iter()
        .map(|vip| {
            let book = store
                .contact(&vip.address)
                .or_log("a VIP's contact could not be read")
                .flatten()
                .and_then(|contact| contact.name);
            Vip {
                name: book.or(vip.name),
                ..vip
            }
        })
        .collect())
}

/// Mark `address` a VIP (`name` the name they wrote with, if any) or take the mark off.
pub fn set(
    store: &dyn Store,
    address: &str,
    name: Option<&str>,
    mark: Mark,
) -> Result<(), CoreError> {
    match mark {
        Mark::Vip => {
            store.put_vip(address, name)?;
        }
        Mark::NotVip => {
            store.delete_vip(address)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_store::MemoryStore;

    #[test]
    fn marking_and_unmarking_goes_through_the_store() {
        let store = MemoryStore::new();
        assert_eq!(toggled(&read(&store), "dana@x.test"), Mark::Vip);
        set(&store, "Dana@X.test", Some("Dana"), Mark::Vip).unwrap();
        assert_eq!(read(&store), Vips::new(["dana@x.test"]));
        assert_eq!(toggled(&read(&store), "dana@x.test"), Mark::NotVip);
        store
            .put_contact("dana@x.test", Some("Dana W"), &mail_store::Origin::Manual)
            .unwrap();
        assert_eq!(
            listed(&store).unwrap(),
            [Vip {
                address: "dana@x.test".to_owned(),
                name: Some("Dana W".to_owned())
            }],
            "the book's name wins"
        );
        set(&store, "dana@x.test", None, Mark::NotVip).unwrap();
        assert!(read(&store).is_empty());
    }
}
