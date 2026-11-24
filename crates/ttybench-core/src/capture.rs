//! A window's capture, of ScreenCaptureKit: a stream of its frames as the
//! window server displays them, each of its display time and of its
//! pixels, the whole window or a band of it, as a tool asks.
//!
//! Its source is the window alone, whatever lies over it, each frame as the
//! window server takes the window's new content; or the screen where the
//! window lies, each frame as the window server composes the display, what
//! the display then shows, the window in front.
//!
//! The capture takes the Screen Recording permission of the app that runs
//! the tool, as macOS grants it to the terminal or the editor that runs a
//! command-line tool: System Settings › Privacy & Security › Screen &
//! System Audio Recording.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
    kCVReturnSuccess,
};
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCFrameStatus, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamFrameInfoDisplayTime, SCStreamFrameInfoStatus, SCStreamOutput, SCStreamOutputType,
    SCWindow,
};

use crate::clock::{mach_to_ns, now_ns};
use crate::frames::{Frame, Rect};

/// How long the capture's start and stop wait for ScreenCaptureKit.
const WAIT: Duration = Duration::from_secs(10);

/// The frames that the capture holds for the tool, at most, before it
/// passes over new ones: some seconds of a display's refreshes.
const HELD: usize = 1024;

/// What the stream's output keeps: where the frames go, and the band of
/// them that the tool asks for.
struct Output {
    frames: Sender<Frame>,
    band: Arc<Mutex<Option<Rect>>>,
    held: Arc<std::sync::atomic::AtomicUsize>,
}

define_class!(
    // SAFETY: `NSObject` asks nothing of a subclass, and this one has no
    // `Drop` of its own.
    #[unsafe(super(NSObject))]
    #[name = "TtybenchCaptureOutput"]
    #[ivars = Output]
    struct StreamOutput;

    // SAFETY: `NSObjectProtocol` asks nothing more of an `NSObject`'s
    // subclass.
    unsafe impl NSObjectProtocol for StreamOutput {}

    // SAFETY: the method's signature is the protocol's.
    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output(
            &self,
            _stream: &SCStream,
            sample: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Screen {
                return;
            }
            let arrived_ns = now_ns();
            let output = self.ivars();
            if output.held.load(std::sync::atomic::Ordering::Relaxed) >= HELD {
                return;
            }
            let Some(display_ns) = displayed(sample) else {
                return;
            };
            // SAFETY: a sample buffer of the screen's output holds an image
            // buffer, which the call retains.
            let Some(buffer) = (unsafe { sample.image_buffer() }) else {
                return;
            };
            let band = *output
                .band
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(frame) = copy(&buffer, band, display_ns, arrived_ns) {
                output
                    .held
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = output.frames.send(frame);
            }
        }
    }
);

impl StreamOutput {
    /// An output of `output`.
    fn new(output: Output) -> Retained<Self> {
        let this = Self::alloc().set_ivars(output);
        // SAFETY: `NSObject`'s `init`, of a subclass that it initializes.
        unsafe { msg_send![super(this), init] }
    }
}

/// The display time of a sample buffer's frame, of a frame that holds new
/// pixels alone: ScreenCaptureKit tells a window that did not change by a
/// frame of the status `idle`, of no pixels.
fn displayed(sample: &CMSampleBuffer) -> Option<u64> {
    // SAFETY: a sample buffer of ScreenCaptureKit's, whose attachments the
    // call reads and retains.
    let attachments = unsafe { sample.sample_attachments_array(false) }?;
    // SAFETY: a sample buffer's attachments are dictionaries, of string
    // keys for ScreenCaptureKit's.
    let attachments = unsafe { attachments.cast_unchecked::<CFDictionary<CFString, CFType>>() };
    let info = attachments.get(0)?;
    // SAFETY: reading constants that ScreenCaptureKit defines.
    let (status, display) = unsafe { (SCStreamFrameInfoStatus, SCStreamFrameInfoDisplayTime) };
    let number = |key: &NSString| -> Option<i64> {
        // SAFETY: an `NSString` is toll-free bridged to a `CFString`.
        let key = unsafe { &*std::ptr::from_ref(key).cast::<CFString>() };
        info.get(key)?.downcast::<CFNumber>().ok()?.as_i64()
    };
    if number(status)? != SCFrameStatus::Complete.0 as i64 {
        return None;
    }
    Some(mach_to_ns(u64::try_from(number(display)?).ok()?))
}

/// A frame of `buffer`'s pixels, BGRA, within `band` if one, of its times.
fn copy(
    buffer: &CVPixelBuffer,
    band: Option<Rect>,
    display_ns: u64,
    arrived_ns: u64,
) -> Option<Frame> {
    // SAFETY: a pixel buffer of the capture, locked to read its pixels.
    if unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) }
        != kCVReturnSuccess
    {
        return None;
    }
    let width = CVPixelBufferGetWidth(buffer);
    let height = CVPixelBufferGetHeight(buffer);
    let stride = CVPixelBufferGetBytesPerRow(buffer);
    let base = CVPixelBufferGetBaseAddress(buffer)
        .cast::<u8>()
        .cast_const();
    let whole = Rect {
        x: 0,
        y: 0,
        width,
        height,
    };
    let area = band.map_or(Some(whole), |band| band.intersect(&whole));
    let frame = area.filter(|_| !base.is_null()).map(|area| {
        let mut pixels = Vec::with_capacity(area.width * area.height * 4);
        for row in area.y..area.bottom() {
            // SAFETY: the row and its columns lie within the buffer's
            // `height` rows of `stride` bytes, of 4 bytes a pixel, which
            // stays locked while it is read.
            let bytes = unsafe {
                std::slice::from_raw_parts(base.add(row * stride + area.x * 4), area.width * 4)
            };
            pixels.extend_from_slice(bytes);
        }
        Frame {
            display_ns,
            arrived_ns,
            area,
            pixels,
        }
    });
    // SAFETY: the lock taken above, of the same flags.
    unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
    frame
}

/// A value that a ScreenCaptureKit completion's thread hands to the
/// tool's.
struct Handed<T>(T);

// SAFETY: ScreenCaptureKit's content, windows and errors are immutable
// descriptions, which its completions hand from its own queues to the
// caller's thread, as its Swift interface declares them sendable.
unsafe impl<T> Send for Handed<T> {}

/// The text of `error`, of none's too.
fn text(error: *mut NSError) -> String {
    // SAFETY: a completion's error, null or an error that it holds while
    // it runs.
    unsafe { error.as_ref() }.map_or_else(
        || "of no error told".to_owned(),
        |error| error.localizedDescription().to_string(),
    )
}

/// What a capture takes its frames of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The window alone, each frame as the window server takes the
    /// window's new content.
    Window,
    /// The screen where the window lies, each frame as the window server
    /// composes the display, as the display shows it.
    Screen,
}

/// A window's capture under way.
pub struct Capture {
    stream: Retained<SCStream>,
    // The output and its queue live as long as the stream gives them
    // frames.
    _output: Retained<StreamOutput>,
    _queue: DispatchRetained<DispatchQueue>,
    frames: Receiver<Frame>,
    band: Arc<Mutex<Option<Rect>>>,
    held: Arc<std::sync::atomic::AtomicUsize>,
    scale: f64,
    size: (usize, usize),
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("scale", &self.scale)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Capture {
    /// Starts the capture of the window numbered `window`, as the window
    /// server lists it, of `source`, of its pixels, as many as its display
    /// has; why not otherwise.
    pub fn start(window: u32, source: Source) -> Result<Self, String> {
        let (window, display) = find(window)?;
        let (filter, area) = match source {
            Source::Window => {
                // SAFETY: a filter of a window that ScreenCaptureKit listed.
                let filter = unsafe {
                    SCContentFilter::initWithDesktopIndependentWindow(
                        SCContentFilter::alloc(),
                        &window.0,
                    )
                };
                (filter, None)
            }
            Source::Screen => {
                // SAFETY: a filter of a display that ScreenCaptureKit listed,
                // of no window left out; plain reads of the frames.
                let (filter, frame, origin) = unsafe {
                    let filter = SCContentFilter::initWithDisplay_excludingWindows(
                        SCContentFilter::alloc(),
                        &display.0,
                        &NSArray::new(),
                    );
                    (filter, window.0.frame(), display.0.frame().origin)
                };
                // The window's frame on its display.
                let area = CGRect {
                    origin: CGPoint {
                        x: frame.origin.x - origin.x,
                        y: frame.origin.y - origin.y,
                    },
                    size: frame.size,
                };
                (filter, Some(area))
            }
        };
        // SAFETY: plain reads of the filter's description of its content.
        let (scale, rect) = unsafe { (f64::from(filter.pointPixelScale()), filter.contentRect()) };
        let rect = area.unwrap_or(rect);
        let size = (
            (rect.size.width * scale).round() as usize,
            (rect.size.height * scale).round() as usize,
        );
        if size.0 == 0 || size.1 == 0 {
            return Err("the window to capture has no size".into());
        }
        // SAFETY: a new configuration, of values that it takes.
        let configuration = unsafe {
            let configuration = SCStreamConfiguration::new();
            configuration.setWidth(size.0);
            configuration.setHeight(size.1);
            configuration.setPixelFormat(kCVPixelFormatType_32BGRA);
            configuration.setShowsCursor(false);
            configuration.setScalesToFit(false);
            configuration.setIgnoreShadowsSingleWindow(true);
            if let Some(area) = area {
                configuration.setSourceRect(area);
            }
            // Frames as often as 240 a second, past the displays' rates;
            // a shorter interval, or none, measured the same.
            configuration.setMinimumFrameInterval(CMTime::new(1, 240));
            configuration.setQueueDepth(8);
            configuration
        };
        // SAFETY: a stream of a filter and a configuration made above, of
        // no delegate.
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &configuration,
                None,
            )
        };
        let (sender, frames) = mpsc::channel();
        let band = Arc::new(Mutex::new(None));
        let held = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let output = StreamOutput::new(Output {
            frames: sender,
            band: Arc::clone(&band),
            held: Arc::clone(&held),
        });
        let queue = DispatchQueue::new("ttybench.capture", None);
        // SAFETY: an output that lives as long as the capture, called on a
        // serial queue of its own.
        unsafe {
            stream.addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*output),
                SCStreamOutputType::Screen,
                Some(&queue),
            )
        }
        .map_err(|error| {
            format!(
                "the capture takes no output: {}",
                text(Retained::as_ptr(&error).cast_mut())
            )
        })?;
        let (started, start) = mpsc::channel();
        let handler = RcBlock::new(move |error: *mut NSError| {
            let _ = started.send((!error.is_null()).then(|| text(error)));
        });
        // SAFETY: a stream made above, of a handler that it calls once.
        unsafe { stream.startCaptureWithCompletionHandler(Some(&handler)) };
        match start.recv_timeout(WAIT) {
            Ok(None) => {}
            Ok(Some(error)) => return Err(format!("the capture does not start: {error}")),
            Err(_) => return Err("the capture did not start within 10 s".into()),
        }
        Ok(Self {
            stream,
            _output: output,
            _queue: queue,
            frames,
            band,
            held,
            scale,
            size,
        })
    }

    /// The window's pixels a point.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// The window's size, in pixels.
    pub fn size(&self) -> (usize, usize) {
        self.size
    }

    /// Takes the frames' band `band` alone, or the whole window of none,
    /// from the next frame.
    pub fn band(&self, band: Option<Rect>) {
        *self
            .band
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = band;
    }

    /// The next frame, within `timeout`.
    pub fn next(&self, timeout: Duration) -> Option<Frame> {
        let frame = self.frames.recv_timeout(timeout).ok()?;
        self.held.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        Some(frame)
    }

    /// The next frame if one came, of no wait.
    pub fn try_next(&self) -> Option<Frame> {
        let frame = self.frames.try_recv().ok()?;
        self.held.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        Some(frame)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let (stopped, stop) = mpsc::channel();
        let handler = RcBlock::new(move |_: *mut NSError| {
            let _ = stopped.send(());
        });
        // SAFETY: the stream started, of a handler that it calls once.
        unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&handler)) };
        let _ = stop.recv_timeout(WAIT);
    }
}

/// A window that ScreenCaptureKit listed, and the display that holds it.
type Found = (Handed<Retained<SCWindow>>, Handed<Retained<SCDisplay>>);

/// ScreenCaptureKit's window numbered `id`, of the windows on screen, and
/// the display that holds its middle, or the first of none.
fn find(id: u32) -> Result<Found, String> {
    let (sender, content) = mpsc::channel();
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: the completion's content, null or one that it holds
            // while it runs, which the call retains.
            let content = unsafe { Retained::retain(content) };
            let _ = sender.send(content.map(Handed).ok_or_else(|| text(error)));
        },
    );
    // SAFETY: a handler that ScreenCaptureKit calls once.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &handler,
        );
    }
    let content = match content.recv_timeout(WAIT) {
        Ok(Ok(content)) => content.0,
        Ok(Err(error)) => {
            return Err(format!(
                "the screen's content cannot be read: {error}; the capture takes the Screen \
                 Recording permission of the app that runs the tool, in System Settings › \
                 Privacy & Security › Screen & System Audio Recording"
            ));
        }
        Err(_) => return Err("the screen's content did not come within 10 s".into()),
    };
    // SAFETY: plain reads of the content's windows and displays.
    let (windows, displays) = unsafe { (content.windows(), content.displays()) };
    let window = windows
        .iter()
        // SAFETY: a plain read of a window's number.
        .find(|window| unsafe { window.windowID() } == id)
        .ok_or_else(|| format!("the window {id} is not on screen to capture"))?;
    // SAFETY: plain reads of the frames.
    let frame = unsafe { window.frame() };
    let middle = (
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let holds = |display: &SCDisplay| {
        // SAFETY: a plain read of a display's frame.
        let place = unsafe { display.frame() };
        middle.0 >= place.origin.x
            && middle.0 < place.origin.x + place.size.width
            && middle.1 >= place.origin.y
            && middle.1 < place.origin.y + place.size.height
    };
    let display = displays
        .iter()
        .find(|display| holds(display))
        .or_else(|| displays.iter().next())
        .ok_or("no display is on to capture")?;
    Ok((Handed(window), Handed(display)))
}
