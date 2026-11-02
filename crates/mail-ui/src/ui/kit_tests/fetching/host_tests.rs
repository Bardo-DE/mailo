//! The link a fetched account starts from, read from a seeded store.

use chrono::{DateTime, TimeZone, Utc};
use mail_client::fetch::{Link, Live};

fn at(second: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap() + chrono::TimeDelta::seconds(second)
}

#[test]
fn mail_with_no_stamp_is_a_fetched_account_current_as_of_now() {
    let (store, _dir) = crate::ui::fixtures::seeded();
    let link = mail_client::schedule::probe(&store, crate::ui::fixtures::acct_account(), at(0));
    assert_eq!(
        link,
        Link::Current {
            at: at(0),
            trouble: vec![],
            live: Live::Polling
        }
    );
}
