//! Icon next to the clock (Windows): the app keeps running there when its
//! window is closed, like other download managers.

#[cfg(windows)]
mod mini;
#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
mod stub;

#[cfg(windows)]
pub use mini::{remove_download, show_download, take_download_actions, update_download};
#[cfg(windows)]
pub use windows::Tray;

#[cfg(not(windows))]
pub use stub::{remove_download, show_download, take_download_actions, update_download, Tray};

#[cfg_attr(not(windows), allow(dead_code))]
pub enum TrayAction {
    Show,
    /// one click on the icon: the window opens, or goes to the tray when it
    /// is already in front
    Toggle,
    AddUrl,
    ResumeAll,
    StopAll,
    ToggleLimiter,
    Scheduler,
    Options,
    About,
    Exit,
}

/// What was clicked on the small icon of a hidden download window.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DownloadAction {
    Show,
    Pause,
}

/// 32 x 32 RGBA picture for a hidden download: a blue "A" with a green
/// bar under it showing how far it is (`None`: size unknown, no bar).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn download_icon(progress: Option<f32>) -> Vec<u8> {
    const N: usize = 32;
    let mut px = vec![0u8; N * N * 4];
    let mut put = |x: usize, y: usize, c: [u8; 4]| {
        let i = (y * N + x) * 4;
        px[i..i + 4].copy_from_slice(&c);
    };
    let blue = [0x1f, 0x6f, 0xeb, 0xff];
    let white = [0xff, 0xff, 0xff, 0xff];
    for y in 0..N {
        for x in 0..N {
            // a square with round corners (radius 6)
            let (dx, dy) = (x.min(N - 1 - x), y.min(N - 1 - y));
            let corner = dx < 6 && dy < 6 && {
                let (cx, cy) = (6.0 - dx as f32 - 0.5, 6.0 - dy as f32 - 0.5);
                cx * cx + cy * cy > 36.0
            };
            if !corner {
                put(x, y, blue);
            }
        }
    }
    let rect = |put: &mut dyn FnMut(usize, usize, [u8; 4]), x0: usize, y0: usize, x1: usize, y1: usize, c: [u8; 4]| {
        for y in y0..y1 {
            for x in x0..x1 {
                put(x, y, c);
            }
        }
    };
    // the letter "A"
    rect(&mut put, 10, 4, 22, 8, white);
    rect(&mut put, 8, 7, 12, 22, white);
    rect(&mut put, 20, 7, 24, 22, white);
    rect(&mut put, 12, 13, 20, 17, white);
    if let Some(p) = progress {
        let p = p.clamp(0.0, 1.0);
        rect(&mut put, 3, 24, 29, 30, [0x0b, 0x2a, 0x5c, 0xff]);
        let end = 4 + ((24.0 * p).round() as usize);
        rect(&mut put, 4, 25, end, 29, [0x3d, 0xdc, 0x5b, 0xff]);
    }
    px
}

/// Windows shows at most 127 characters in an icon's hint.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn short_tip(tip: &str) -> String {
    if tip.chars().count() <= 120 {
        tip.to_string()
    } else {
        tip.chars().take(119).chain(Some('…')).collect()
    }
}

/// The bar changes in steps of 1/24 (its width in dots).
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn step_of(p: f32) -> u32 {
    (p.clamp(0.0, 1.0) * 24.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_icon_shows_the_progress() {
        let empty = download_icon(Some(0.0));
        let half = download_icon(Some(0.5));
        assert_eq!(empty.len(), 32 * 32 * 4);
        let green = |px: &[u8], x: usize| px[(27 * 32 + x) * 4 + 1] == 0xdc;
        assert!(!green(&empty, 10) && green(&half, 10) && !green(&half, 26));
        // corners are see-through, the letter is white
        assert_eq!(half[3], 0);
        assert_eq!(&half[(5 * 32 + 10) * 4..(5 * 32 + 10) * 4 + 4], &[255, 255, 255, 255]);
        assert_eq!(short_tip(&"x".repeat(300)).chars().count(), 120);
        assert_eq!(step_of(0.5), 12);
    }
}
