use crate::i18n::{t, tf};
use crate::*;

const TIPS: [&str; 14] = [
    "Copy a download link anywhere (Ctrl+C) and Aknan Download offers to download it.",
    "Play a video on a web page and click \"Download this video\" above it to choose the quality.",
    "Right click a web page in Chrome or Edge and choose \"Download all links with Aknan Download\" to pick files from the page.",
    "Every download uses up to 32 connections. If a server is slow with many, add it to Options > Connection > Exceptions.",
    "Make a queue (Queues > New queue) and set \"1 file at the same time\" to download files one after another.",
    "The Scheduler can start a queue at night and shut down the computer when everything is finished.",
    "Downloads > When all downloads finish lets you shut down, sleep or hibernate the computer after the last file.",
    "The speed limiter on the toolbar keeps your internet usable for other things while downloading.",
    "Add Batch > \"Address with a pattern\" downloads img001.jpg … img100.jpg from one address with a *.",
    "An old link expired? Right click the download > \"Refresh download address\" and continue from where it stopped.",
    "Right click a category in the sidebar > Properties to change its file types and its own folder.",
    "View > Columns shows more columns, like Date Added, Save To and Referer.",
    "Tasks > Run site grabber collects every image, video or archive of a web site.",
    "Hold Alt while clicking a link in the browser to let the browser download it by itself.",
];

/// The tip at start, once no message box is open (they must not mix).
pub(crate) fn show_tip_when_free(ui: Ui) {
    if modal_open() {
        slint::Timer::single_shot(std::time::Duration::from_millis(700), move || show_tip_when_free(ui));
        return;
    }
    show_tip(&ui, false);
}

pub(crate) fn show_tip(ui: &Ui, from_menu: bool) {
    let Some(d) = ui.tips.upgrade() else { return };
    let mut s = ui.ctx.mgr.settings();
    let i = s.tip_index as usize % TIPS.len();
    d.set_tip(t(TIPS[i]).into());
    d.set_counter(tf("Tip {} of {}", &[&(i + 1), &TIPS.len()]).into());
    d.set_show_on_start(s.show_tips);
    if !from_menu {
        // the next start shows the next tip
        s.tip_index = ((i + 1) % TIPS.len()) as u32;
        let _ = ui.ctx.mgr.set_settings(s);
    }
    open_window!(d);
}

pub(super) fn wire(ui: &Ui) {
    let Some(d) = ui.tips.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_next(move || {
            let mut s = ui.ctx.mgr.settings();
            s.tip_index = (s.tip_index + 1) % TIPS.len() as u32;
            let _ = ui.ctx.mgr.set_settings(s);
            show_tip(&ui, true);
        });
    }
    {
        let ui = ui.clone();
        d.on_close_window(move || {
            let Some(d) = ui.tips.upgrade() else { return };
            let mut s = ui.ctx.mgr.settings();
            if s.show_tips != d.get_show_on_start() {
                s.show_tips = d.get_show_on_start();
                let _ = ui.ctx.mgr.set_settings(s);
            }
            let _ = d.hide();
        });
    }
}
