use super::AfterDone;
use std::path::Path;
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}
pub fn file_system(_dir: &Path) -> Option<String> {
    None
}
pub fn power(_action: AfterDone, _force: bool) {}
pub fn idle_secs() -> Option<u64> {
    None
}
pub struct WakeHandle;
pub fn wake_at(_at: chrono::DateTime<chrono::Utc>) -> Option<WakeHandle> {
    None
}
