use iced::futures::{
    StreamExt,
    channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
    stream,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

define_class!(
    // SAFETY: NSObject has no subclassing requirements, Delegate has no Drop, and
    // an application delegate is only ever touched on the main thread.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustyPaintOpenWithDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            for url in urls {
                if let Some(path) = url.to_file_path() {
                    send(path);
                }
            }
        }
    }
);

// winit promises never to register a delegate of its own, and wants the shared
// application asked for only once its event loop exists.
pub fn watch(_window: &dyn iced::window::Window) {
    static WATCHING: OnceLock<()> = OnceLock::new();
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if WATCHING.set(()).is_err() {
        return;
    }

    let delegate: Retained<Delegate> = unsafe { msg_send![mtm.alloc::<Delegate>(), init] };
    NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // NSApplication holds its delegate weakly, so this one has to outlive the call.
    std::mem::forget(delegate);
}

pub fn later() -> iced::Subscription<PathBuf> {
    iced::Subscription::run(opened)
}

fn opened() -> impl stream::Stream<Item = PathBuf> {
    stream::unfold(receiver(), |mut waiting| async move {
        let path = waiting.next().await?;
        Some((path, waiting))
    })
}

// The delegate fires from AppKit rather than from the runtime, so the two meet at a channel.
fn channel() -> &'static Mutex<Option<UnboundedSender<PathBuf>>> {
    static CHANNEL: OnceLock<Mutex<Option<UnboundedSender<PathBuf>>>> = OnceLock::new();
    CHANNEL.get_or_init(|| Mutex::new(None))
}

fn receiver() -> UnboundedReceiver<PathBuf> {
    let (send, receive) = unbounded();
    if let Ok(mut slot) = channel().lock() {
        *slot = Some(send);
    }
    receive
}

fn send(path: PathBuf) {
    if let Ok(slot) = channel().lock()
        && let Some(sender) = slot.as_ref()
    {
        let _ = sender.unbounded_send(path);
    }
}
