//! `/room` (every room control, for members) and `/rooms` (setup, off and
//! status, for admins). The panel's buttons do the same things; both go
//! through `rooms::ops::act`.
pub mod admin;
pub mod room;

/// Both commands.
pub fn commands() -> Vec<crate::Command> {
    vec![room::room(), admin::rooms()]
}
