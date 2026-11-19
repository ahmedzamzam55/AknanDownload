use super::redraw::repaint_all;
use slint::ComponentHandle;

/// Where a window of `size` goes to be in the middle of a screen at `screen`
/// (position) with `screen_size`; never above or left of the screen.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn centered(screen: (i32, i32), screen_size: (u32, u32), size: (u32, u32)) -> (i32, i32) {
    let x = screen.0 + (screen_size.0 as i32 - size.0 as i32).max(0) / 2;
    let y = screen.1 + (screen_size.1 as i32 - size.1 as i32).max(0) / 2;
    (x, y)
}

/// Where every window is made, and where a window waits before it is shown
/// again: far from every screen. Windows puts a new window on the screen
/// before its first picture is drawn (white for a moment, in its own corner
/// on the left, then it jumped to the middle). Away from the screen nobody
/// sees that; `place` brings the window in once it is drawn.
#[cfg(windows)]
const AWAY: (i32, i32) = (-30000, -30000);

/// The window attributes hook of the backend (main): windows are made away.
#[cfg(windows)]
pub(crate) fn made_away(
    a: slint::winit_030::winit::window::WindowAttributes,
) -> slint::winit_030::winit::window::WindowAttributes {
    a.with_position(slint::winit_030::winit::dpi::PhysicalPosition::new(AWAY.0, AWAY.1))
}

/// A hidden window that is shown again waits away from the screen too.
#[cfg(windows)]
pub(crate) fn send_away(w: &slint::Window) {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|win: &winit::window::Window| {
        if !win.is_maximized() && win.is_visible() != Some(true) {
            win.set_outer_position(winit::dpi::PhysicalPosition::new(AWAY.0, AWAY.1));
        }
    });
}

#[cfg(not(windows))]
pub(crate) fn send_away(_w: &slint::Window) {}

#[cfg(windows)]
fn has_system_window(w: &slint::Window) -> bool {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|_: &winit::window::Window| ()).is_some()
}

#[cfg(not(windows))]
fn has_system_window(_w: &slint::Window) -> bool {
    true
}

#[cfg(windows)]
fn place_now(w: &slint::Window, maximizable: bool) {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|win: &winit::window::Window| place_winit(win, maximizable));
}

#[cfg(windows)]
fn place_winit(win: &slint::winit_030::winit::window::Window, maximizable: bool) {
    use slint::winit_030::winit;
    use winit::window::WindowButtons;
    if !win.is_decorated() {
        // a window drawn without the Windows frame: its buttons are not
        // touched (that gives it back a hidden Windows title bar)
    } else if maximizable {
        // the main window: its maximize button (full screen)
        win.set_resizable(true);
        win.set_enabled_buttons(WindowButtons::all());
    } else {
        win.set_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE);
        if win.is_maximized() {
            win.set_maximized(false);
        }
    }
    if win.is_maximized() {
        // it fills its screen already
        return;
    }
    // the screen of the mouse (where the user is working), else primary, else current,
    // else the OS work area so a window can NEVER remain stranded off-screen
    let monitor = cursor_monitor(win).or_else(|| win.primary_monitor()).or_else(|| win.current_monitor());
    let o = win.outer_size();
    let (x, y) = if let Some(m) = monitor {
        let (p, s) = (m.position(), m.size());
        centered((p.x, p.y), (s.width, s.height), (o.width, o.height))
    } else {
        let (pos, sz) = fallback_work_area();
        centered(pos, sz, (o.width, o.height))
    };
    win.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
}

#[cfg(windows)]
fn fallback_work_area() -> ((i32, i32), (u32, u32)) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    let mut rc = RECT { left: 0, top: 0, right: 1280, bottom: 720 };
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rc as *mut RECT as *mut _, 0);
    }
    let w = (rc.right - rc.left).max(400) as u32;
    let h = (rc.bottom - rc.top).max(300) as u32;
    ((rc.left, rc.top), (w, h))
}

/// The screen the mouse pointer is on.
#[cfg(windows)]
fn cursor_monitor(win: &slint::winit_030::winit::window::Window) -> Option<slint::winit_030::winit::monitor::MonitorHandle> {
    let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    // SAFETY: writes the pointer position into `pt`
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt) } == 0 {
        return None;
    }
    win.available_monitors().find(|m| {
        let (p, s) = (m.position(), m.size());
        pt.x >= p.x && pt.x < p.x + s.width as i32 && pt.y >= p.y && pt.y < p.y + s.height as i32
    })
}

#[cfg(not(windows))]
fn place_now(_w: &slint::Window, _maximizable: bool) {}

/// Time for the first picture of a window that was just put on its place
/// away from the screen.
const FIRST_PICTURE: std::time::Duration = std::time::Duration::from_millis(70);

/// A window was just opened (away from the screen): once it is drawn it
/// comes to the middle of the screen and, except for the main window,
/// loses its maximize button.
pub(crate) fn place<C>(weak: slint::Weak<C>, maximizable: bool)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    place_then(weak, maximizable, || {});
}

/// The same, and `then` once the window is on the screen.
pub(crate) fn place_then<C>(weak: slint::Weak<C>, maximizable: bool, then: impl FnOnce() + 'static)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    wait_for_picture(weak, maximizable, Box::new(then), 0, false);
}

fn wait_for_picture<C>(weak: slint::Weak<C>, maximizable: bool, then: Box<dyn FnOnce()>, tries: u32, seen: bool)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    // the system window is made a moment after show(): when it is there,
    // one more moment for its first picture
    let wait = if tries == 0 || seen { FIRST_PICTURE } else { std::time::Duration::from_millis(20) };
    slint::Timer::single_shot(wait, move || {
        let Some(c) = weak.upgrade() else { return };
        if !c.window().is_visible() {
            return;
        }
        let there = has_system_window(c.window());
        if there && (seen || tries == 0) {
            super::redraw::mark_placed(c.window());
            place_now(c.window(), maximizable);
            repaint_all(&c);
            c.window().request_redraw();
            then();
        } else if there {
            wait_for_picture(weak, maximizable, then, tries + 1, true);
        } else if tries < 250 {
            wait_for_picture(weak, maximizable, then, tries + 1, false);
        } else {
            then();
        }
    });
}
