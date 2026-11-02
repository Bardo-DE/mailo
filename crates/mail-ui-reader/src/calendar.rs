//! The calendar mailo puts invitations in, and where it comes from.
//!
//! The same two questions as the mail-link handler: *what* the window says about it is the
//! UI's; *where* the calendar is comes from a seam ([`Desk`]), which is the platform's in the
//! launched window ([`platform`]) and a fake in the tests, which must never reach a real
//! calendar. The calendar itself is [`porter_calendar::CalendarStore`], the trait every porter
//! client shares; [`Desk`] is the same four calls made object-safe so one can be a root context.
//!
//! What each platform does:
//! - macOS: EventKit, through `mail-appkit` (the one crate that may use `unsafe`). The system
//!   asks the person for access the first time a calendar is read or written.
//! - Elsewhere: [`handoff::Handoff`], which writes the event as an `.ics` file and hands it to
//!   the default calendar application. It cannot list calendars or find or remove an event.

pub mod flow;
pub mod handoff;

use std::sync::Arc;

pub use mail_ui_kit::calendar::{Boxed, Calendars, Desk};

/// This platform's calendar: EventKit on macOS, the default calendar application elsewhere.
pub fn platform() -> Option<Calendars> {
    #[cfg(target_os = "macos")]
    {
        Some(Calendars(Arc::new(mail_appkit::EventKit::new())))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(Calendars(Arc::new(handoff::Handoff::new(
            handoff::default_application(),
        ))))
    }
}
