//! Smaller windows: Columns, Category properties, Find, File Properties,
//! Duplicate link, shut down countdown, Tip of the day, Refresh download
//! address, and Export / Import of the list.

mod category;
mod columns;
mod dup_power;
mod export;
mod find;
mod props;
mod refresh;
mod tips;
mod whats_new;

pub(crate) use category::{delete_category, open_category};
pub(crate) use columns::open_columns;
pub(crate) use dup_power::{ask_duplicate, start_countdown, tick_countdown};
pub(crate) use export::{export_list, import_list};
pub(crate) use find::{find_next, open_find};
pub(crate) use props::open_properties;
pub(crate) use refresh::{open_refresh, refresh_received};
pub(crate) use tips::{show_tip, show_tip_when_free};
#[cfg(test)]
pub(crate) use whats_new::WHATS_NEW;
pub(crate) use whats_new::{av1_tip, finished_well, whats_new};

use crate::Ui;

pub(crate) fn wire(ui: &Ui) {
    columns::wire(ui);
    category::wire(ui);
    find::wire(ui);
    props::wire(ui);
    dup_power::wire(ui);
    tips::wire(ui);
    refresh::wire(ui);
}
