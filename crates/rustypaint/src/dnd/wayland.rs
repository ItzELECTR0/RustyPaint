use iced::futures::{
    StreamExt,
    channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
    stream,
};
use std::io::Read;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    backend::Backend,
    delegate_noop, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_data_device::{self, WlDataDevice},
        wl_data_device_manager::{DndAction, WlDataDeviceManager},
        wl_data_offer::{self, WlDataOffer},
        wl_registry::{self, WlRegistry},
        wl_seat::WlSeat,
    },
};

const URI_LIST: &str = "text/uri-list";

pub fn watch(window: &dyn iced::window::Window) {
    use iced::window::raw_window_handle::RawDisplayHandle;

    let Ok(handle) = window.display_handle() else {
        return;
    };
    let RawDisplayHandle::Wayland(wayland) = handle.as_raw() else {
        return;
    };

    static WATCHING: OnceLock<()> = OnceLock::new();
    if WATCHING.set(()).is_err() {
        return;
    }

    let display = wayland.display.as_ptr() as usize;
    let _ = std::thread::Builder::new()
        .name("wayland-drops".into())
        .spawn(move || listen(display));
}

pub fn drops() -> iced::Subscription<PathBuf> {
    iced::Subscription::run(|| match posted().1.lock().unwrap().take() {
        Some(dropped) => dropped.boxed(),
        None => stream::pending().boxed(),
    })
}

type Posted = (
    UnboundedSender<PathBuf>,
    Mutex<Option<UnboundedReceiver<PathBuf>>>,
);

fn posted() -> &'static Posted {
    static POSTED: OnceLock<Posted> = OnceLock::new();
    POSTED.get_or_init(|| {
        let (to, from) = unbounded();
        (to, Mutex::new(Some(from)))
    })
}

// The connection belongs to winit; a guest backend gives us our own queue on it.
fn listen(display: usize) {
    let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
    let connection = Connection::from_backend(backend);
    let Ok((globals, mut queue)) = registry_queue_init::<Drops>(&connection) else {
        return;
    };
    let handle = queue.handle();
    let (Ok(seat), Ok(manager)) = (
        globals.bind::<WlSeat, _, _>(&handle, 1..=1, ()),
        globals.bind::<WlDataDeviceManager, _, _>(&handle, 1..=3, ()),
    ) else {
        return;
    };

    let _device = manager.get_data_device(&seat, &handle, ());
    let mut drops = Drops {
        connection: connection.clone(),
        offer: None,
        post: posted().0.clone(),
    };
    while queue.blocking_dispatch(&mut drops).is_ok() {}
}

struct Drops {
    connection: Connection,
    offer: Option<WlDataOffer>,
    post: UnboundedSender<PathBuf>,
}

#[derive(Default)]
struct Offered {
    mimes: Mutex<Vec<String>>,
    matched: AtomicBool,
}

impl Dispatch<WlDataDevice, ()> for Drops {
    fn event(
        state: &mut Self,
        _device: &WlDataDevice,
        event: wl_data_device::Event,
        _data: &(),
        connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::Enter {
                serial,
                id: Some(offer),
                ..
            } => {
                if offers_uris(&offer) {
                    offer.accept(serial, Some(URI_LIST.to_owned()));
                    if offer.version() >= 3 {
                        offer.set_actions(DndAction::Copy, DndAction::Copy);
                    }
                    state.offer = Some(offer);
                } else {
                    offer.accept(serial, None);
                    offer.destroy();
                }
                let _ = connection.flush();
            }
            wl_data_device::Event::Leave => {
                if let Some(offer) = state.offer.take() {
                    offer.destroy();
                    let _ = connection.flush();
                }
            }
            wl_data_device::Event::Drop => {
                if let Some(offer) = state.offer.take() {
                    state.take(offer);
                }
            }
            wl_data_device::Event::Selection { id: Some(offer) } => offer.destroy(),
            _ => {}
        }
    }

    event_created_child!(Drops, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, Offered::default()),
    ]);
}

impl Dispatch<WlDataOffer, Offered> for Drops {
    fn event(
        _state: &mut Self,
        _offer: &WlDataOffer,
        event: wl_data_offer::Event,
        offered: &Offered,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_offer::Event::Offer { mime_type } => {
                offered.mimes.lock().unwrap().push(mime_type);
            }
            wl_data_offer::Event::Action { dnd_action } => {
                let chosen = dnd_action.into_result().unwrap_or(DndAction::None);
                offered
                    .matched
                    .store(chosen != DndAction::None, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Drops {
    fn event(
        _state: &mut Self,
        _registry: &WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(Drops: ignore WlSeat);
delegate_noop!(Drops: WlDataDeviceManager);

impl Drops {
    // The source writes the list down a pipe, so read it off the event queue's thread.
    fn take(&mut self, offer: WlDataOffer) {
        let Ok((reader, writer)) = std::io::pipe() else {
            offer.destroy();
            return;
        };
        offer.receive(URI_LIST.to_owned(), writer.as_fd());
        drop(writer);
        let _ = self.connection.flush();

        let connection = self.connection.clone();
        let post = self.post.clone();
        let _ = std::thread::Builder::new()
            .name("wayland-drop-read".into())
            .spawn(move || {
                let mut uris = String::new();
                let read = { reader }.read_to_string(&mut uris);
                finish(&offer);
                let _ = connection.flush();
                if read.is_ok() {
                    for path in paths(&uris) {
                        let _ = post.unbounded_send(path);
                    }
                }
            });
    }
}

fn finish(offer: &WlDataOffer) {
    let matched = offer
        .data::<Offered>()
        .is_some_and(|offered| offered.matched.load(Ordering::Relaxed));
    if offer.version() >= 3 && matched {
        offer.finish();
    }
    offer.destroy();
}

fn offers_uris(offer: &WlDataOffer) -> bool {
    offer
        .data::<Offered>()
        .is_some_and(|offered| offered.mimes.lock().unwrap().iter().any(|m| m == URI_LIST))
}

fn paths(uris: &str) -> Vec<PathBuf> {
    uris.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(local_path)
        .collect()
}

fn local_path(uri: &str) -> Option<PathBuf> {
    let authority = uri.strip_prefix("file://")?;
    let path = &authority[authority.find('/')?..];
    Some(PathBuf::from(unescape(path)))
}

fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escape = (bytes[i] == b'%')
            .then(|| Some((digit(*bytes.get(i + 1)?)?, digit(*bytes.get(i + 2)?)?)))
            .flatten();
        match escape {
            Some((high, low)) => {
                out.push(high << 4 | low);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crlf_uri_list_becomes_paths() {
        let list = "file:///home/me/one.png\r\nfile:///home/me/two.png\r\n";
        assert_eq!(
            paths(list),
            vec![
                PathBuf::from("/home/me/one.png"),
                PathBuf::from("/home/me/two.png"),
            ]
        );
    }

    #[test]
    fn escapes_and_hosts_and_comments_are_understood() {
        let list = "#comment\nfile://localhost/tmp/a%20b%25c.png\n";
        assert_eq!(paths(list), vec![PathBuf::from("/tmp/a b%c.png")]);
    }

    #[test]
    fn anything_that_is_not_a_local_file_is_left_alone() {
        assert!(paths("https://example.invalid/cat.png\n").is_empty());
        assert!(paths("").is_empty());
    }
}
