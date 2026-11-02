use mail_client::message::Frames;
use mail_client::testing::Store;
use mail_domain::Message;
use mail_reader::RemoteImages;
use mail_reader::SanitizePolicy;

#[test]
fn warming_a_conversation_makes_opening_it_a_lookup() {
    let (client, _dir) = crate::ui::fixtures::realistic();
    let thread = crate::ui::fixtures::thread_like(&client, "rust-lang/rust");
    let messages: Vec<Message> = client
        .thread(thread)
        .unwrap()
        .messages
        .iter()
        .map(|id| client.message(*id).unwrap())
        .collect();
    let policy = SanitizePolicy::FRAME;
    assert!(
        messages.iter().any(|m| m.body.raw().is_some()),
        "the fixture has bodies"
    );
    let frames = Frames::plain();
    frames.warm_now(&client, &[thread], policy, || true);
    for message in messages.iter().filter(|m| m.body.raw().is_some()) {
        let had = frames.peek(message, policy).expect("warmed");
        assert_eq!(*had, frames.uncached(&client, message, policy));
    }
}

#[test]
fn warming_that_is_no_longer_wanted_stops() {
    let (client, _dir) = crate::ui::fixtures::realistic();
    let thread = crate::ui::fixtures::thread_like(&client, "rust-lang/rust");
    let message = client
        .message(client.thread(thread).unwrap().messages[0])
        .unwrap();
    let policy = SanitizePolicy {
        remote_images: RemoteImages::Allowed,
        version: u32::MAX,
        ..SanitizePolicy::FRAME
    };
    let frames = Frames::plain();
    frames.warm_now(&client, &[thread], policy, || false);
    assert_eq!(frames.peek(&message, policy), None);
}
