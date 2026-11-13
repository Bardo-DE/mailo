//! The window's end of the handoff: reading what the running window is asked.
//!
//! The bus, the asks and the token are the application's own (`crate::handoff`), shared with
//! `mailo open` and the intent provider, which hand a conversation to the window and have no
//! window of their own. This is only the part that draws: it reads the queue on the thread that
//! draws and shows each ask as a click on its row would.

use dioxus::prelude::*;
use mail_ui_kit::space::Handle;
use mail_ui_kit::view::Shell;

/// Read what the running window is asked, on the thread that draws: show each ask in this
/// window (`shell`, as a click on its row would), in a Space that shows its account (`spaces`
/// switches to it), do what it asks there, and raise the window with the ask's activation token.
/// Called once, from `App`; a window with no [`Requests`] reads nothing.
///
/// [`Requests`]: crate::handoff::Requests
#[cfg(not(windows))]
pub(in crate::ui) fn use_handoff(shell: Signal<Shell>, spaces: Handle) {
    use crate::handoff::{ActivationToken, Requests};
    use crate::ui::ask::Window;

    let Some(requests) = try_consume_context::<Requests>() else {
        return;
    };
    // The Dock icon clicked again reaches the queue through the application delegate. It can only
    // be set on the main thread once winit has made `NSApplication`, which is by the time `App`
    // first draws. (URLs the system opens are heard earlier, in `serve`.)
    #[cfg(target_os = "macos")]
    use_hook({
        let requests = requests.clone();
        move || {
            if !crate::handoff::hear_reopen(&requests) {
                log::warn!("the Dock icon is not heard: no application delegate");
            }
        }
    });
    let window = ds_blitz::use_window_handle();
    let took = use_hook(|| try_consume_context::<Took>().unwrap_or_default());
    use_future(move || {
        let requests = requests.clone();
        let window = window.clone();
        let took = took.clone();
        async move {
            let Some(mut queue) = requests.take() else {
                return;
            };
            took.0.set(true);
            while let Some(open) = queue.recv().await {
                let here = Window { shell, spaces };
                if open.ask.arrives_first() {
                    here.arrive(&*open.ask);
                }
                open.ask.run(here);
                if let Some(window) = &window {
                    window.focus_with_token(open.token.map(ActivationToken::into_string));
                }
            }
        }
    });
}

/// Where there is no bus and no socket nothing is served, so there is nothing to read.
#[cfg(windows)]
pub(in crate::ui) fn use_handoff(_shell: Signal<Shell>, _spaces: Handle) {}

/// Whether this main window took the request queue ([`use_handoff`] marks it).
#[derive(Clone, Default)]
pub(in crate::ui) struct Took(std::rc::Rc<std::cell::Cell<bool>>);

/// For the main window's root, above the damaged settings' question: a window that closes
/// without ever reading the queue (closed at that question, or before its first frame) says so,
/// or the queue would wait for it for ever and the Dock icon would open nothing again.
pub(in crate::ui) fn use_window_coming() {
    let took = use_context_provider(Took::default);
    #[cfg(target_os = "macos")]
    {
        let requests = use_hook(try_consume_context::<crate::handoff::Requests>);
        use_drop(move || {
            if let Some(requests) = &requests
                && !took.0.get()
            {
                requests.gone_before_taking();
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = took;
}
