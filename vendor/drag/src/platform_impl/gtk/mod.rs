// Copyright 2023-2023 CrabNebula Ltd.
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::{CursorPosition, DragItem, DragMode, DragResult, Error, Image, Options};
use gdkx11::{
    gdk,
    glib::{self, ObjectExt, Propagation, SignalHandlerId, WeakRef},
};
use gtk::{
    gdk_pixbuf,
    prelude::{
        DeviceExt, DragContextExtManual, PixbufLoaderExt, SeatExt, WidgetExt, WidgetExtManual,
    },
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
};

type Handlers = Arc<Mutex<Vec<SignalHandlerId>>>;

thread_local! {
    // The signal handlers of the last drag, disconnected when the next one
    // starts (GTK runs on the main thread only). A weak reference, so that
    // the window can still be destroyed in between.
    static LAST_DRAG: RefCell<Option<(WeakRef<gtk::ApplicationWindow>, Handlers)>> = const { RefCell::new(None) };
}

pub fn start_drag<F: Fn(DragResult, CursorPosition) + Send + 'static>(
    window: &gtk::ApplicationWindow,
    item: DragItem,
    image: Image,
    on_drop_callback: F,
    options: Options,
) -> crate::Result<()> {
    log::debug!("Starting drag operation with mode: {:?}", options.mode);
    if let Some((last_window, last_handlers)) = LAST_DRAG.take() {
        if let Some(last_window) = last_window.upgrade() {
            clear_signal_handlers(&last_window, &mut last_handlers.lock().unwrap());
        }
    }
    let handler_ids: Handlers = Arc::new(Mutex::new(vec![]));
    LAST_DRAG.set(Some((window.downgrade(), handler_ids.clone())));
    let drag_action = match options.mode {
        DragMode::Copy => gdk::DragAction::COPY,
        DragMode::Move => gdk::DragAction::MOVE,
    };

    log::debug!("Setting drag source with action: {:?}", drag_action);
    window.drag_source_set(gdk::ModifierType::BUTTON1_MASK, &[], drag_action);

    match item {
        DragItem::Files(paths) => {
            log::debug!("Setting up file drag with {} paths", paths.len());
            window.drag_source_add_uri_targets();
            handler_ids
                .lock()
                .unwrap()
                .push(window.connect_drag_data_get(move |_, _, data, _, _| {
                    log::debug!("Preparing URIs for drag data");
                    // A bare `file://` + path breaks on `#`, `?`, `%` and spaces:
                    // the receiver gets a path to a file that does not exist.
                    let uris: Vec<String> = paths
                        .iter()
                        .filter_map(|path| glib::filename_to_uri(path, None).ok())
                        .map(|uri| uri.to_string())
                        .collect();
                    let uris: Vec<&str> = uris.iter().map(|s| s.as_str()).collect();
                    log::debug!("Setting URIs: {:?}", uris);
                    data.set_uris(&uris);
                }));
        }
        DragItem::Data { .. } => {
            // Currently leaving it as is as we can utilize it as a dummy dragging feature
            // on_drop_callback(DragResult::Cancel, get_cursor_position(window).unwrap());
            // return Ok(());
        }
    }

    if let Some(target_list) = &window.drag_source_get_target_list() {
        log::debug!("Got target list, initiating drag");
        if let Some(drag_context) = window.drag_begin_with_coordinates(
            target_list,
            drag_action,
            gdk::ffi::GDK_BUTTON1_MASK as i32,
            None,
            -1,
            -1,
        ) {
            log::debug!("Drag context created successfully");
            let result = Rc::new(Cell::new(DragResult::Cancel));
            on_drop_failed(result.clone(), window, &handler_ids, &options);
            on_drop_performed(result.clone(), &drag_context);
            on_drag_end(on_drop_callback, result, window, &handler_ids);

            log::debug!("Setting up drag icon");
            let icon_pixbuf: Option<gdk_pixbuf::Pixbuf> = match &image {
                Image::Raw(data) => image_binary_to_pixbuf(data),
                Image::File(path) => match std::fs::read(path) {
                    Ok(bytes) => image_binary_to_pixbuf(&bytes),
                    Err(_) => None,
                },
            };
            if let Some(icon) = icon_pixbuf {
                drag_context.drag_set_icon_pixbuf(&icon, 0, 0);
            }

            Ok(())
        } else {
            Err(crate::Error::FailedToStartDrag)
        }
    } else {
        Err(crate::Error::EmptyTargetList)
    }
}

fn image_binary_to_pixbuf(data: &[u8]) -> Option<gdk_pixbuf::Pixbuf> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader
        .write(data)
        .and_then(|_| loader.close())
        .map_err(|_| ())
        .and_then(|_| loader.pixbuf().ok_or(()))
        .ok()
}

fn clear_signal_handlers(window: &gtk::ApplicationWindow, handler_ids: &mut Vec<SignalHandlerId>) {
    for handler_id in handler_ids.drain(..) {
        window.disconnect(handler_id);
    }
}

fn on_drop_failed(
    result: Rc<Cell<DragResult>>,
    window: &gtk::ApplicationWindow,
    handler_ids: &Handlers,
    options: &Options,
) {
    log::debug!("Setting up drop failed handler");
    let skip_animatation_on_cancel_or_failure = options.skip_animatation_on_cancel_or_failure;

    handler_ids
        .lock()
        .unwrap()
        .push(window.connect_drag_failed(move |_, _, _drag_result| {
            log::debug!("Drag failed or cancelled");
            result.set(DragResult::Cancel);
            if skip_animatation_on_cancel_or_failure {
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        }));
}

fn on_drop_performed(result: Rc<Cell<DragResult>>, drag_context: &gdk::DragContext) {
    log::debug!("Setting up drop performed handler");
    drag_context.connect_drop_performed(move |context, _| {
        log::debug!("Drop performed successfully");
        log::trace!("Selected action: {:?}", context.selected_action());
        log::trace!("Suggested action: {:?}", context.suggested_action());
        result.set(DragResult::Dropped);
    });
}

// The receiver may ask for the data (`drag-data-get`) after `drop-performed`
// and even after `drag-end` (GTK 3 on Wayland): the handlers stay connected
// until the next drag starts, otherwise nothing is dropped.
// https://github.com/crabnebula-dev/drag-rs/issues/84
fn on_drag_end<F: Fn(DragResult, CursorPosition) + Send + 'static>(
    callback: F,
    result: Rc<Cell<DragResult>>,
    window: &gtk::ApplicationWindow,
    handler_ids: &Handlers,
) {
    log::debug!("Setting up drag end handler");
    // Called once, then dropped: the handler outlives the drag, and the
    // callback holds Tauri objects that must not be dropped while the window
    // is being destroyed (Tauri then panics on a RefCell already borrowed).
    let callback = RefCell::new(Some(callback));
    handler_ids
        .lock()
        .unwrap()
        .push(window.connect_drag_end(move |window, _| {
            log::debug!("Drag ended");
            // Only stops the window from starting drags by itself.
            window.drag_source_unset();
            if let Some(callback) = callback.take() {
                callback(result.get(), get_cursor_position(window).unwrap());
            }
        }));
}

fn get_cursor_position(window: &gtk::ApplicationWindow) -> Result<CursorPosition, Error> {
    if let Some(cursor) = window
        .display()
        .default_seat()
        .and_then(|seat| seat.pointer())
    {
        let (_, x, y) = cursor.position();
        Ok(CursorPosition { x, y })
    } else {
        Err(Error::FailedToGetCursorPosition)
    }
}
