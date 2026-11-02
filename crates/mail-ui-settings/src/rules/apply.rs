//! Apply Rules on picked conversations: asks before it sends anything.
//!
//! The rules that reply or forward speak for the person. Applying them to mail the person picked
//! is their ask, but not their ask to send, so when a rule of an account in the picks would send,
//! an alert says so first and offers to apply the rules without the sending.

use super::work::apply_to;
use dioxus::prelude::*;
use ds::components::overlays::alert_model::{AlertButton, AlertRole, AlertStyle};
use ds::prelude::*;
use mail_client::Client;
use mail_client::rules::manage::Sending;
use mail_domain::ThreadId;
use mail_ui_kit::view::Shell;
use mail_ui_model::copy;

/// Apply the rules to `threads`: at once when none of them would send, else ask first.
pub fn apply_or_ask(client: &Client, mut shell: Signal<Shell>, threads: Vec<ThreadId>) {
    if client.rules_manage().would_send(&threads) {
        shell.write().applying_rules = Some(threads);
    } else {
        say(&apply_to(
            client,
            &threads,
            Sending::Yes,
            chrono::Utc::now(),
        ));
    }
}

fn say(said: &str) {
    mail_ui_kit::motion::tell(said.to_owned(), mail_ui_kit::motion::Follow::Nothing);
}

/// Close the alert, applying nothing, and give the keyboard back to the window.
pub fn close(mut shell: Signal<Shell>) {
    shell.write().applying_rules = None;
    mail_ui_kit::host::Host::focus_app();
}

fn answer(shell: Signal<Shell>, sending: Sending) {
    let Some(threads) = shell.peek().applying_rules.clone() else {
        return;
    };
    close(shell);
    let client = mail_ui_kit::live::client();
    say(&apply_to(&client, &threads, sending, chrono::Utc::now()));
}

/// The alert while it is open: its Cancel answers Escape.
#[component]
pub fn ApplyRulesAlert(shell: Signal<Shell>) -> Element {
    if shell.read().applying_rules.is_none() {
        return rsx! {};
    }
    rsx! {
        Alert {
            title: copy::confirm::APPLY_RULES_SENDS.to_owned(),
            message: Some(TextLine::from(copy::confirm::APPLY_RULES_SENDS_BODY)),
            style: AlertStyle::Warning,
            buttons: vec![
                AlertButton::new(
                    copy::controls::APPLY_AND_SEND,
                    AlertRole::Normal,
                    EventHandler::new(move |()| answer(shell, Sending::Yes)),
                ),
                AlertButton::new(
                    copy::controls::APPLY_WITHOUT_SENDING,
                    AlertRole::Normal,
                    EventHandler::new(move |()| answer(shell, Sending::No)),
                ),
                AlertButton::new(
                    copy::controls::CANCEL,
                    AlertRole::Cancel,
                    EventHandler::new(move |()| close(shell)),
                ),
            ],
        }
    }
}
