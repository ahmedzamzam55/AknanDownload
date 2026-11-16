//! User settings, stored as JSON in the app data folder.

mod defaults;
mod methods;
mod schedule;
mod settings;
#[cfg(test)]
mod tests;
mod types;

pub use methods::{host_matches, parse_hhmm, wildcard_match};
pub use schedule::{AfterDone, QueueSchedule, Schedule};
pub use settings::Settings;
pub use types::{
    default_categories, default_column_width, CategoryDef, QueueDef, ServerLimit, SiteLogin,
    ALL_COLUMNS, DEFAULT_CAPTURE_EXTS, DEFAULT_COLUMNS, DEFAULT_PORT, DEFAULT_USER_AGENT,
    MAIN_QUEUE,
};
