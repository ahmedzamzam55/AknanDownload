use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;

thread_local! {
    static PLACED_UNTIL: RefCell<HashMap<usize, Instant>> = RefCell::new(HashMap::new());
}

#[cfg(windows)]
pub(super) fn mark_placed(w: &slint::Window) {
    let key = w as *const slint::Window as usize;
    let until = Instant::now() + std::time::Duration::from_millis(500);
    PLACED_UNTIL.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|_, v| *v > Instant::now());
        m.insert(key, until);
    });
}

#[cfg(windows)]
fn in_place_grace(key: usize) -> bool {
    PLACED_UNTIL.with(|m| m.borrow().get(&key).is_some_and(|t| *t > Instant::now()))
}

/// Draw all of `c` again at the next frame (not only what changed).
pub(crate) fn repaint_all<C>(c: &C)
where
    C: ComponentHandle,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    let t = c.global::<crate::Theme>();
    t.set_repaint(t.get_repaint().wrapping_add(1));
}

/// The windows are drawn by the processor, and only the parts that changed
/// are drawn again. When Windows throws away what a window showed (it was
/// hidden next to the clock, the screen slept, the size or the scale
/// changed...), the rest stayed black. So the whole window is drawn again
/// whenever that can have happened, and at most once a second whenever
/// Windows asks for a picture. `dropped`: files dropped on the window.
pub(crate) fn keep_drawn<C>(c: &C)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    repaint_all(c);
    hook_window(c, None);
}

/// The same, with files dropped on the window from Explorer.
pub(crate) fn keep_drawn_with_drop<C>(c: &C, dropped: impl Fn(std::path::PathBuf) + 'static)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    repaint_all(c);
    hook_window(c, Some(Box::new(dropped)));
}

/// `w.keep_drawn()` on any window (a reference to one too).
pub(crate) trait KeepDrawn {
    fn keep_drawn(&self);
}

impl<C> KeepDrawn for C
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    fn keep_drawn(&self) {
        keep_drawn(self);
    }
}

thread_local! {
    // the window (by its address) whose handler also takes dropped files:
    // it is not replaced by one without them
    static WITH_DROP: Cell<usize> = const { Cell::new(0) };
}

#[cfg(windows)]
fn hook_window<C>(c: &C, dropped: Option<Box<dyn Fn(std::path::PathBuf)>>)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    use super::kbd_sync;
    use super::shortcuts::TranslatedShortcut;
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    use winit::event::WindowEvent;
    use winit::keyboard::KeyCode;
    // one handler per window (a new one replaces the old): the one that
    // also takes dropped files stays, and keyboard shortcut normalization
    // lives inside this same handler so keep_drawn never overwrites it
    let key = c.window() as *const slint::Window as usize;
    if dropped.is_some() {
        WITH_DROP.with(|w| w.set(key));
    } else if WITH_DROP.with(|w| w.get()) == key {
        return;
    }
    let weak = c.as_weak();
    let last = Cell::new(std::time::Instant::now());
    let tracked_mods = Cell::new(kbd_sync::OsMods::poll().0);
    let active_translated: Cell<Option<(KeyCode, TranslatedShortcut)>> = Cell::new(None);
    c.window().on_winit_window_event(move |win, ev| {
        let all = |weak: &slint::Weak<C>| {
            if let Some(c) = weak.upgrade() {
                repaint_all(&c);
            }
        };
        match ev {
            WindowEvent::DroppedFile(p) => {
                if let Some(f) = &dropped {
                    f(p.clone());
                }
            }
            WindowEvent::Focused(false) => {
                active_translated.set(None);
                kbd_sync::release_all(win, &tracked_mods);
            }
            WindowEvent::Focused(true) => {
                last.set(std::time::Instant::now());
                all(&weak);
                kbd_sync::sync_from_os(win, &tracked_mods, kbd_sync::OsMods::poll(), true);
            }
            WindowEvent::ModifiersChanged(_) => {
                let os = kbd_sync::OsMods::poll();
                if !os.alt() {
                    kbd_sync::release_stale_alt(win, &tracked_mods);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(res) = kbd_sync::handle_keyboard_input(win, event, &tracked_mods, &active_translated) {
                    return res;
                }
            }
            // a window comes onto the screen from where it waited (see
            // `place`): Windows kept nothing of what was drawn there, and
            // only the parts that changed were drawn again, so it stayed
            // white around its buttons
            WindowEvent::Moved(_) if in_place_grace(key) || last.get().elapsed() > std::time::Duration::from_millis(150) => {
                last.set(std::time::Instant::now());
                all(&weak);
                if let Some(c) = weak.upgrade() {
                    c.window().request_redraw();
                }
            }
            WindowEvent::Resized(_)
            | WindowEvent::Occluded(false)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::ThemeChanged(_) => {
                last.set(std::time::Instant::now());
                all(&weak);
            }
            // Windows asks for a picture (or we did): the whole window now
            // and then, so nothing stays black for long
            WindowEvent::RedrawRequested if in_place_grace(key) || last.get().elapsed() > std::time::Duration::from_secs(1) => {
                last.set(std::time::Instant::now());
                all(&weak);
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

#[cfg(not(windows))]
fn hook_window<C>(c: &C, _dropped: Option<Box<dyn Fn(std::path::PathBuf)>>)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    let _ = c;
    let _ = &WITH_DROP;
}
