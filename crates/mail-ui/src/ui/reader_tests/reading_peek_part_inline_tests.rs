use mail_ui_reader::reading::peek::part::{LEFT_OPEN_FOR, OPEN_PREFIX, sweep_in, write_for_open};
use mail_ui_kit::downloads::unedited;
use mail_core::Store as _;
use mail_ui_kit::view::Viewing;
use mail_ui_model::settings::RemoveUnedited;
use std::time::SystemTime;

/// Open writes a copy of the part and records it: the quit removes it, unedited, under When
/// Mailo Quits. Without the record the copy stays for ever.
#[test]
fn a_copy_open_writes_is_recorded_for_the_quit_to_remove() {
    let _alone = unedited::RUN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (store, _dir) = crate::ui::fixtures::held_and_remote();
    let client = crate::ui::fixtures::client(&store);
    let thread = crate::ui::fixtures::thread_like(&store, "quarterly");
    let message = store.thread(thread).expect("the thread").messages[0];
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let root = scratch.path().join(format!("{OPEN_PREFIX}test"));
    std::fs::create_dir_all(&root).expect("Open's folder");

    let path = write_for_open(&client, &root, Viewing::of(message, 0)).expect("written");
    assert!(path.starts_with(&root) && path.exists(), "{path:?}");
    unedited::remove_on_quit(RemoveUnedited::WhenMailoQuits);
    assert!(!path.exists(), "the copy Open wrote was not recorded");
}

/// A folder an earlier run's Open left, once old enough, is cleared away; a new one, a file
/// named like one, and anything else in the temporary folder are left.
#[test]
fn old_folders_open_left_are_swept_and_nothing_else() {
    let temp = tempfile::tempdir().expect("a scratch folder");
    let left = temp.path().join(format!("{OPEN_PREFIX}abc"));
    std::fs::create_dir_all(left.join("1234")).expect("a folder Open made");
    std::fs::write(left.join("1234").join("plans.pdf"), b"%PDF").expect("its file");
    let file = temp.path().join(format!("{OPEN_PREFIX}file"));
    std::fs::write(&file, b"x").expect("a file named like one");
    let other = temp.path().join("someone-else");
    std::fs::create_dir(&other).expect("another folder");

    // Now: all of it is too new.
    sweep_in(temp.path(), SystemTime::now());
    assert!(left.exists(), "a folder from now was swept");

    let later = SystemTime::now() + LEFT_OPEN_FOR * 2;
    sweep_in(temp.path(), later);
    assert!(!left.exists(), "an old folder was left");
    assert!(file.exists() && other.exists(), "something else was swept");
}
