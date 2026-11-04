//! An invitation said in words, and what becomes of its event in a calendar: which calendar an
//! event goes into, and what an answer did to the event already stored. Putting an event in a
//! calendar store, and taking it out, are `mail_core::invite`'s.

pub mod when;

use porter_calendar::{CalendarRef, Stored};

pub use when::{REPEATS_OTHERWISE, WhenSaid, repeats_words, say_when};

/// Which of `calendars` an event goes into: the one named `wanted` while it is there and takes
/// events, else the first that does. `None` when none does.
pub fn pick<'a>(calendars: &'a [CalendarRef], wanted: &str) -> Option<&'a CalendarRef> {
    calendars
        .iter()
        .find(|calendar| calendar.writable && calendar.id == wanted)
        .or_else(|| calendars.iter().find(|calendar| calendar.writable))
}

/// What an answer did to the calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Followed {
    /// The event is in the calendar, as the store now holds it.
    Kept(Stored),
    /// A declined event that was in the calendar is gone from it.
    Removed,
    /// A declined event that was never in the calendar: nothing to do.
    Absent,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calendar(id: &str, writable: bool) -> CalendarRef {
        CalendarRef {
            id: id.to_owned(),
            name: id.to_owned(),
            color: None,
            writable,
        }
    }

    #[test]
    fn the_chosen_calendar_is_used_while_it_takes_events_else_the_first_that_does() {
        let calendars = [
            calendar("holidays", false),
            calendar("work", true),
            calendar("home", true),
        ];
        // (wanted, the calendar picked)
        let cases = [
            ("home", Some("home")),
            ("", Some("work")),
            ("gone", Some("work")),
            ("holidays", Some("work")),
        ];
        for (wanted, picked) in cases {
            assert_eq!(
                pick(&calendars, wanted).map(|c| c.id.as_str()),
                picked,
                "{wanted:?}"
            );
        }
        assert!(pick(&[calendar("holidays", false)], "").is_none());
        assert!(pick(&[], "").is_none());
    }
}
