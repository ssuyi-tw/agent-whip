//! System-tray icon + menu (ports the Electron `Tray`). Left-click spawns/drops
//! the whip; right-click opens a menu with Quit. Tray and menu events are
//! forwarded into the winit event loop as [`UserEvent`]s.

use crate::UserEvent;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::event_loop::EventLoopProxy;

const ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icon/whip-tray.png"
));

/// Keeps the tray icon alive for the lifetime of the app.
pub struct Tray {
    _tray: TrayIcon,
}

pub fn build(proxy: EventLoopProxy<UserEvent>) -> Tray {
    let img = image::load_from_memory(ICON_PNG)
        .expect("decode tray icon")
        .to_rgba8();
    let (w, h) = img.dimensions();
    let icon = Icon::from_rgba(img.into_raw(), w, h).expect("build tray icon");

    let menu = Menu::new();
    let quit = MenuItem::new("Quit", true, None);
    menu.append(&quit).expect("append quit item");
    let quit_id = quit.id().clone();

    // Forward menu selections.
    {
        let proxy = proxy.clone();
        let quit_id = quit_id.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == quit_id {
                let _ = proxy.send_event(UserEvent::Quit);
            }
        }));
    }

    // Forward tray left-clicks.
    {
        let proxy = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                let _ = proxy.send_event(UserEvent::TrayToggle);
            }
        }));
    }

    #[allow(unused_mut)]
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("agent-whip — click for whip")
        .with_icon(icon);
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_icon_as_template(true);
    }

    let tray = builder.build().expect("build tray icon");
    Tray { _tray: tray }
}
