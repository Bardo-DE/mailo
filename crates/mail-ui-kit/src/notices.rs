//! Where the window says new mail arrived and a reminder came back.

use dioxus::prelude::ReadableExt;
use mail_client::notify::Notifier;
use std::sync::Arc;

/// Where the window says new mail arrived and a reminder came back: the desktop's notifications
/// in the launched window, a recorder in a test. A window with none says it only in its list.
///
/// Provided whether or not notifications are on; [`speaking`] asks the setting each time, so
/// turning them off or on in Settings takes effect at once.
#[derive(Clone)]
pub struct Notices(pub Arc<dyn Notifier>);

/// The window's notices while `notifications.new_mail` is on, read now. A window with no settings
/// (a test's lone component) has them on, as the setting's default is.
pub fn speaking(
    notices: Option<&Notices>,
    settings: Option<dioxus::prelude::Signal<mail_ui_model::settings::MailSettings>>,
) -> Option<Arc<dyn Notifier>> {
    let on = settings.is_none_or(|settings| {
        settings.peek().notifications.new_mail == mail_ui_model::settings::NewMail::On
    });
    notices.filter(|_| on).map(|notices| notices.0.clone())
}

impl std::fmt::Debug for Notices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Notices(..)")
    }
}
