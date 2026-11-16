use super::{DownloadAction, TrayAction};

pub fn show_download(_id: u64, _tip: &str, _progress: Option<f32>, _labels: (&str, &str)) -> bool {
    false
}
pub fn update_download(_id: u64, _tip: &str, _progress: Option<f32>) {}
pub fn remove_download(_id: u64) {}
pub fn take_download_actions() -> Vec<(u64, DownloadAction)> {
    Vec::new()
}

pub struct Tray;
impl Tray {
    pub fn new() -> Option<Tray> {
        None
    }
    pub fn poll(&self) -> Vec<TrayAction> {
        Vec::new()
    }
    pub fn set_limiter(&self, _on: bool) {}
    pub fn refresh_labels(&self) {}
}
