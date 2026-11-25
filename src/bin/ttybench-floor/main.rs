//! `ttybench-floor`: the least that an app of AppKit does to show a window,
//! the floor of a terminal's startup, which `ttybench startup --floor`
//! launches as the terminal launches and measures as it measures the
//! terminal. Of `--bundle <folder>`, it makes its app's bundle there, of
//! its own executable, for a launch by `open`, and prints its path.
//!
//! ```text
//! ttybench-floor [--bundle <folder>]
//! ```

use std::process::ExitCode;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use ttybench::cli::Args;
use ttybench::floor;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("ttybench-floor: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(std::env::args().skip(1), &["bundle"], &[])?;
    if let Some(folder) = args.value("bundle") {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let bundle = floor::bundle(std::path::Path::new(folder), &executable)
            .map_err(|error| format!("{folder}: {error}"))?;
        println!("{}", bundle.display());
        return Ok(());
    }
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // SAFETY: an object of a class of no instance variables, made of its
    // `init` on the main thread.
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(mtm), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let _window = window(mtm);
    app.activate();
    app.run();
    Ok(())
}

define_class!(
    /// The floor's delegate, which takes the documents that a launch by
    /// `open` gives, as a terminal takes its script, and opens none: AppKit
    /// would tell otherwise that the floor cannot open them.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TtybenchFloorDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, _urls: &NSArray<NSURL>) {}
    }
);

/// The floor's one window, of a terminal's common size, in front.
fn window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let frame = NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(800.0, 500.0));
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    // SAFETY: a window of a frame, a style and a buffered backing, made on
    // the main thread, which the app keeps as it runs.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is the app's, which outlives it.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("ttybench floor"));
    window.makeKeyAndOrderFront(None);
    window
}
