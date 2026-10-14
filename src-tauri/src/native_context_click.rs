//! Record a real right-click on a page webview.
//!
//! The page can call `browser_runtime_report` whenever it wants. That call is
//! not proof of a click. This module listens to the native mouse event, which
//! page script cannot send, and tells the shell that this tab was clicked.

use std::sync::Arc;

use tauri::Webview;

pub fn arm(webview: &Webview, page_id: usize, on_click: Arc<dyn Fn() + Send + Sync>) {
    let on_click = Arc::clone(&on_click);
    if let Err(error) = webview.with_webview(move |platform| connect(platform, page_id, on_click)) {
        tracing::warn!("could not watch native right-clicks: {error}");
    }
}

pub fn forget(page_id: usize) {
    forget_view(page_id);
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn connect(
    platform: tauri::webview::PlatformWebview,
    _page_id: usize,
    on_click: Arc<dyn Fn() + Send + Sync>,
) {
    use gtk::prelude::WidgetExt;
    use webkit2gtk::WebViewExt;

    let view = platform.inner();
    let on_press = Arc::clone(&on_click);
    view.connect_button_press_event(move |_view, event| {
        // Button 3 is the right button. The page cannot synthesize this event.
        if event.button() == 3 {
            on_press();
        }
        gtk::glib::Propagation::Proceed
    });
    // Also fires for the keyboard menu key. A DOM `contextmenu` event does not.
    view.connect_context_menu(move |_view, _menu, _event, _hit| {
        on_click();
        false
    });
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn forget_view(_page_id: usize) {}

#[cfg(target_os = "macos")]
fn connect(
    platform: tauri::webview::PlatformWebview,
    page_id: usize,
    on_click: Arc<dyn Fn() + Send + Sync>,
) {
    watch_view(page_id, platform.inner(), on_click);
}

#[cfg(target_os = "macos")]
fn forget_view(page_id: usize) {
    watched().retain(|entry| entry.page_id != page_id);
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "macos"
)))]
fn connect(
    _platform: tauri::webview::PlatformWebview,
    _page_id: usize,
    _on_click: Arc<dyn Fn() + Send + Sync>,
) {
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "macos"
)))]
fn forget_view(_page_id: usize) {}

#[cfg(target_os = "macos")]
struct Watched {
    page_id: usize,
    /// Does not keep the webview alive. `load` is `None` after it is freed.
    view: objc2::rc::Weak<objc2_app_kit::NSView>,
    on_click: Arc<dyn Fn() + Send + Sync>,
}

#[cfg(target_os = "macos")]
// SAFETY: Entries are inserted on the main thread, where `with_webview` runs.
// The click monitor also runs there, and it is the only place that loads the
// view and calls AppKit. Other threads only add or remove entries.
// Objective-C weak load and destroy are safe to interleave. The callback is
// already `Send`.
unsafe impl Send for Watched {}

#[cfg(target_os = "macos")]
fn watched() -> std::sync::MutexGuard<'static, Vec<Watched>> {
    use std::sync::Mutex;

    static WATCHED: Mutex<Vec<Watched>> = Mutex::new(Vec::new());
    WATCHED.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(target_os = "macos")]
fn watch_view(page_id: usize, view: *mut std::ffi::c_void, on_click: Arc<dyn Fn() + Send + Sync>) {
    use std::sync::Once;

    use objc2::rc::{Retained, Weak};
    use objc2_app_kit::NSView;

    // WKWebView is an NSView. Retain only long enough to register a weak
    // reference, then drop it so closing the page can free the view.
    let Some(retained) = (unsafe { Retained::retain(view as *mut NSView) }) else {
        return;
    };
    let view = Weak::from_retained(&retained);
    drop(retained);
    {
        let mut watched = watched();
        watched.retain(|entry| entry.page_id != page_id);
        watched.push(Watched {
            page_id,
            view,
            on_click,
        });
    }

    static MONITOR: Once = Once::new();
    MONITOR.call_once(|| {
        use std::ptr::NonNull;

        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};

        let block = block2::RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event_ref = unsafe { event.as_ref() };
            if is_context_click(event_ref) {
                note_hit_views(event_ref);
            }
            event.as_ptr()
        });
        let mask = NSEventMask::RightMouseDown | NSEventMask::LeftMouseDown;
        let block_ref: &block2::DynBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> = &block;
        if let Some(monitor) =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, block_ref) }
        {
            // Dropping the monitor removes the hook. It has to live for the process.
            std::mem::forget(monitor);
            std::mem::forget(block);
        }

        fn is_context_click(event: &NSEvent) -> bool {
            let kind = event.r#type();
            kind == NSEventType::RightMouseDown
                || (kind == NSEventType::LeftMouseDown
                    && event
                        .modifierFlags()
                        .contains(NSEventModifierFlags::Control))
        }

        fn note_hit_views(event: &NSEvent) {
            use objc2::rc::Retained;

            let Some(marker) = MainThreadMarker::new() else {
                return;
            };
            let Some(event_window) = event.window(marker) else {
                return;
            };
            let point = event.locationInWindow();
            // Clone matches, then drop the guard. A callback may re-enter
            // this list (arm or forget), and must not run while it is locked.
            let callbacks = {
                let mut watched = watched();
                let mut callbacks = Vec::new();
                watched.retain(|entry| {
                    let Some(view) = entry.view.load() else {
                        return false;
                    };
                    if view.isHiddenOrHasHiddenAncestor() {
                        return true;
                    }
                    let Some(view_window) = view.window() else {
                        return true;
                    };
                    if Retained::as_ptr(&event_window) != Retained::as_ptr(&view_window) {
                        return true;
                    }
                    let local = view.convertPoint_fromView(point, None);
                    if view.mouse_inRect(local, view.bounds()) {
                        callbacks.push(Arc::clone(&entry.on_click));
                    }
                    true
                });
                callbacks
            };
            for on_click in callbacks {
                on_click();
            }
        }
    });
}
