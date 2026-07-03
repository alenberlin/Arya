//! Calendar context for meetings (macOS EventKit).
//!
//! Read-only: finds the event overlapping "now" (or starting soon) so
//! recordings get real titles and attendees, and the UI can prompt before a
//! meeting starts. Requires the user's one-time Calendar access grant;
//! everything degrades gracefully without it.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub title: String,
    pub attendees: Vec<String>,
    /// Minutes from now until the event starts (negative = already started).
    pub starts_in_min: i64,
    pub ends_in_min: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarAccess {
    Granted,
    Denied,
    NotDetermined,
    #[cfg(not(target_os = "macos"))]
    Unsupported,
}

#[cfg(target_os = "macos")]
pub use macos::{access_status, current_or_upcoming_event, request_access};

#[cfg(not(target_os = "macos"))]
pub fn access_status() -> CalendarAccess {
    CalendarAccess::Unsupported
}

#[cfg(not(target_os = "macos"))]
pub fn request_access() -> CalendarAccess {
    CalendarAccess::Unsupported
}

#[cfg(not(target_os = "macos"))]
pub fn current_or_upcoming_event(_within_min: i64) -> Option<CalendarEvent> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEvent, EKEventStore};
    use objc2_foundation::{NSArray, NSDate};

    use super::{CalendarAccess, CalendarEvent};

    pub fn access_status() -> CalendarAccess {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        match status {
            EKAuthorizationStatus::FullAccess => CalendarAccess::Granted,
            EKAuthorizationStatus::Denied | EKAuthorizationStatus::Restricted => {
                CalendarAccess::Denied
            }
            EKAuthorizationStatus::NotDetermined => CalendarAccess::NotDetermined,
            _ => CalendarAccess::Denied,
        }
    }

    /// Fires the system prompt if undetermined; blocks briefly for the answer.
    pub fn request_access() -> CalendarAccess {
        if access_status() == CalendarAccess::Granted {
            return CalendarAccess::Granted;
        }
        let store = unsafe { EKEventStore::new() };
        let (tx, rx) = mpsc::channel::<bool>();
        let block = RcBlock::new(
            move |granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| {
                let _ = tx.send(granted.as_bool());
            },
        );
        let block_ptr = &*block as *const block2::Block<_>
            as *mut block2::Block<dyn Fn(objc2::runtime::Bool, *mut objc2_foundation::NSError)>;
        unsafe { store.requestFullAccessToEventsWithCompletion(block_ptr) };
        match rx.recv_timeout(std::time::Duration::from_secs(120)) {
            Ok(true) => CalendarAccess::Granted,
            Ok(false) => CalendarAccess::Denied,
            Err(_) => access_status(),
        }
    }

    /// The event overlapping now, or the next one starting within
    /// `within_min` minutes. Prefers already-running events.
    pub fn current_or_upcoming_event(within_min: i64) -> Option<CalendarEvent> {
        if access_status() != CalendarAccess::Granted {
            return None;
        }
        let store = unsafe { EKEventStore::new() };
        let now = NSDate::date();
        let window_start = NSDate::dateWithTimeIntervalSinceNow(-6.0 * 3600.0);
        let window_end = NSDate::dateWithTimeIntervalSinceNow(within_min as f64 * 60.0);
        let predicate = unsafe {
            store.predicateForEventsWithStartDate_endDate_calendars(
                &window_start,
                &window_end,
                None,
            )
        };
        let events: Retained<NSArray<EKEvent>> =
            unsafe { store.eventsMatchingPredicate(&predicate) };

        let mut best: Option<(f64, CalendarEvent)> = None;
        for event in events.iter() {
            if unsafe { event.isAllDay() } {
                continue;
            }
            let start = unsafe { event.startDate() };
            let end = unsafe { event.endDate() };
            let starts_in = start.timeIntervalSinceDate(&now) / 60.0;
            let ends_in = end.timeIntervalSinceDate(&now) / 60.0;
            if ends_in <= 0.0 || starts_in > within_min as f64 {
                continue;
            }
            let title = unsafe { event.title() }.to_string();
            if title.is_empty() {
                continue;
            }
            let attendees = unsafe { event.attendees() }
                .map(|list| {
                    list.iter()
                        .filter_map(|participant| {
                            unsafe { participant.name() }.map(|n| n.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();
            let candidate = CalendarEvent {
                title,
                attendees,
                starts_in_min: starts_in.round() as i64,
                ends_in_min: ends_in.round() as i64,
            };
            // Running events (starts_in <= 0) beat upcoming; then soonest.
            let rank = if starts_in <= 0.0 {
                starts_in.abs() - 10_000.0
            } else {
                starts_in
            };
            if best.as_ref().map(|(r, _)| rank < *r).unwrap_or(true) {
                best = Some((rank, candidate));
            }
        }
        best.map(|(_, event)| event)
    }
}
