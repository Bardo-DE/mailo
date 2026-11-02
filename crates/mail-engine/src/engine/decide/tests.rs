use super::*;
use chrono::TimeZone;
use mail_domain::SyncCursor;

fn at(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
}

fn imap(validity: u32, next: u32) -> SyncCursor {
    SyncCursor::Imap {
        uidvalidity: validity,
        uidnext: next,
        modseq: Some(7),
    }
}

fn uid(n: u32) -> RemoteRef {
    RemoteRef::Imap {
        mailbox: "INBOX".to_owned(),
        uidvalidity: 1,
        uid: n,
    }
}

fn thread(n: u128) -> ThreadId {
    ThreadId::from_uuid(uuid::Uuid::from_u128(n))
}

const EVERY: Duration = Duration::from_secs(900);

fn state(held: Option<SyncCursor>, cursor: Option<SyncCursor>, swept: Option<i64>) -> MailboxState {
    MailboxState {
        held_through: held,
        cursor,
        expunges_swept: swept.map(at),
    }
}

#[test]
fn a_survey_resumes_only_from_a_state_that_means_something_in_the_mailbox() {
    let held = imap(1, 50);
    let cases: Vec<(&str, MailboxState, Option<SyncCursor>)> = vec![
        (
            "held through a state, recently listed whole",
            state(Some(held.clone()), Some(imap(1, 50)), Some(0)),
            Some(held.clone()),
        ),
        (
            "nothing held yet",
            state(None, Some(imap(1, 50)), Some(0)),
            None,
        ),
        (
            "not an IMAP state",
            state(Some(SyncCursor::Pop), None, Some(0)),
            None,
        ),
        (
            "a validity of zero is the server not saying",
            state(Some(imap(0, 50)), None, Some(0)),
            None,
        ),
        (
            "a next UID of zero is the server not saying",
            state(Some(imap(1, 0)), None, Some(0)),
            None,
        ),
        (
            "the server numbered the mailbox afresh since",
            state(Some(held.clone()), Some(imap(2, 9)), Some(0)),
            None,
        ),
        (
            "no cursor stored is not a reset",
            state(Some(held.clone()), None, Some(0)),
            Some(held.clone()),
        ),
        (
            "the whole listing for deletions is due",
            state(Some(held.clone()), Some(imap(1, 50)), Some(-900)),
            None,
        ),
        (
            "and was never made",
            state(Some(held.clone()), Some(imap(1, 50)), None),
            None,
        ),
        (
            "not yet due, a second short of the interval",
            state(Some(held.clone()), Some(imap(1, 50)), Some(-899)),
            Some(held),
        ),
    ];
    for (name, state, want) in cases {
        assert_eq!(resume_point(&state, EVERY, at(0)), want, "{name}");
        assert_eq!(
            survey_kind(&state, EVERY, at(0)),
            want.map_or(Survey::Whole, Survey::From),
            "{name}"
        );
    }
}

#[test]
fn a_resumed_survey_is_made_again_whole_when_the_server_numbers_the_mailbox_afresh() {
    let resumed = Survey::From(imap(1, 50));
    assert!(resumed.resumed());
    assert!(!resumed.redo(Some(&imap(1, 60))));
    assert!(resumed.redo(Some(&imap(2, 3))));
    assert!(!resumed.redo(None), "a survey with no cursor says nothing");
    // A whole listing has nothing to be wrong about.
    assert!(!Survey::Whole.redo(Some(&imap(2, 3))));
    assert!(!Survey::Whole.resumed());
    assert_eq!(Survey::Whole.since(), FetchSince::Beginning);
    assert_eq!(
        resumed.since(),
        FetchSince::After {
            cursor: imap(1, 50)
        }
    );
}

#[test]
fn nothing_is_new_only_when_a_resumed_survey_left_the_cursor_and_listed_what_is_held() {
    let (a, b) = (Some(imap(1, 50)), Some(imap(1, 51)));
    let resumed = Survey::From(imap(1, 50));
    assert!(may_have_found_nothing(&resumed, &a, &a));
    assert!(!may_have_found_nothing(&resumed, &b, &a));
    assert!(!may_have_found_nothing(&Survey::Whole, &a, &a));

    let held: HashSet<RemoteRef> = [uid(1), uid(2)].into();
    assert!(all_held(&[(uid(2), 5)], &held));
    assert!(all_held(&[], &held));
    assert!(!all_held(&[(uid(2), 5), (uid(3), 5)], &held));
}

#[test]
fn headers_are_wanted_newest_first_less_what_is_held_or_resting_the_smaller_band_ahead() {
    let kb = 1024;
    // Server order: oldest first.
    let listed = vec![
        (uid(1), 10 * kb),
        (uid(2), 5 * kb * kb),
        (uid(3), 10 * kb),
        (uid(4), 200 * kb),
        (uid(5), 10 * kb),
        (uid(6), u64::MAX),
    ];
    let held: HashSet<RemoteRef> = [uid(1)].into();
    let resting: HashSet<RemoteRef> = [uid(5)].into();
    let wanted = headers_wanted(listed, &held, &resting);
    assert_eq!(
        wanted.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(),
        [uid(3), uid(4), uid(6), uid(2)]
    );
}

#[test]
fn a_batch_is_one_band_in_order_and_no_more_than_is_left() {
    let wanted = vec![
        (uid(1), 10),
        (uid(2), 20),
        (uid(3), 500),
        (uid(4), 30),
        (uid(5), u64::MAX),
    ];
    assert_eq!(header_batch(&wanted, 100, 10), [uid(1), uid(2), uid(4)]);
    assert_eq!(header_batch(&wanted, 100, 2), [uid(1), uid(2)]);
    assert_eq!(header_batch(&wanted, 100, 0), Vec::<RemoteRef>::new());
    assert_eq!(header_batch(&wanted, u64::MAX, 10).len(), 5);
}

#[test]
fn everything_listed_is_held_through_where_the_cursor_stands_without_its_modseq() {
    let through = Some(SyncCursor::Imap {
        uidvalidity: 1,
        uidnext: 50,
        modseq: None,
    });
    assert_eq!(held_mark(&Some(imap(1, 50)), &None), through);
    assert_eq!(held_mark(&Some(imap(1, 50)), &Some(imap(1, 40))), through);
    // Remembered already, whatever the cursor's modseq has since become.
    assert_eq!(held_mark(&Some(imap(1, 50)), &through), None);
    assert_eq!(held_mark(&Some(SyncCursor::Pop), &None), None);
    assert_eq!(held_mark(&None, &None), None);
}

fn backlog(hinted: &[(u128, u32)], rest: &[u32]) -> Backlog {
    Backlog {
        hinted: hinted.iter().map(|(t, n)| (thread(*t), uid(*n))).collect(),
        rest: rest.iter().map(|n| uid(*n)).collect(),
    }
}

#[test]
fn the_asked_for_conversations_come_first_and_the_window_s_own_fetches_are_left_out() {
    let wanted = Wanted {
        asked: vec![thread(2), thread(1)],
        taken: vec![uid(20)],
    };
    let none = HashSet::new();
    let (order, first) = bodies_wanted(
        backlog(&[(1, 10), (2, 20), (2, 21)], &[30, 10, 31]),
        &wanted,
        &none,
        10,
    );
    assert_eq!(order, [uid(21), uid(10), uid(30), uid(31)]);
    assert_eq!(first, 2, "the claimed one was in front, and is not counted");
}

#[test]
fn nothing_asked_is_the_backlog_as_the_store_gave_it() {
    let (order, first) = bodies_wanted(
        backlog(&[], &[3, 2, 1]),
        &Wanted::default(),
        &HashSet::new(),
        2,
    );
    assert_eq!(order, [uid(3), uid(2)]);
    assert_eq!(first, 0);
}

#[test]
fn those_that_could_not_be_read_do_not_keep_a_place_in_the_limit() {
    // The store is asked for the limit and one more, since one rests.
    let resting: HashSet<RemoteRef> = [uid(3)].into();
    assert_eq!(backlog_asked(2, &resting), 3);
    let wanted = Wanted {
        asked: vec![thread(1)],
        taken: vec![],
    };
    let (order, first) = bodies_wanted(backlog(&[(1, 3)], &[2, 1, 0]), &wanted, &resting, 2);
    assert_eq!(order, [uid(2), uid(1)]);
    assert_eq!(first, 0, "the only wanted one rests");
}

#[test]
fn a_size_the_survey_did_not_give_is_unknown() {
    let sizes: HashMap<RemoteRef, u64> = [(uid(1), 5)].into();
    assert_eq!(
        sized(vec![uid(1), uid(2)], &sizes),
        [(uid(1), 5), (uid(2), u64::MAX)]
    );
}
