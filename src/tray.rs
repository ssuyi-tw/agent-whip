//! System-tray icon + menu (ports the Electron `Tray`). Left-click spawns/drops
//! the whip; right-click opens a menu to toggle the crack action/sound/roar,
//! pick a whip skin, and Quit. Tray and menu events are forwarded into the winit
//! event loop as [`UserEvent`]s.

use crate::UserEvent;
use crate::skins::Skin;
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::event_loop::EventLoopProxy;

const ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icon/whip-tray.png"
));

/// Keeps the tray icon alive for the lifetime of the app, plus the skin radio
/// items so the app can move the checkmark when the skin changes.
pub struct Tray {
    _tray: TrayIcon,
    skin_items: Vec<CheckMenuItem>,
}

impl Tray {
    /// Check the selected skin and uncheck the rest (the menu is radio-style,
    /// but muda has no radio group, so we drive the checkmarks ourselves).
    pub fn select_skin(&self, index: usize) {
        for (i, item) in self.skin_items.iter().enumerate() {
            item.set_checked(i == index);
        }
    }
}

pub fn build(proxy: EventLoopProxy<UserEvent>, skins: &[Skin], current: usize) -> Tray {
    let img = image::load_from_memory(ICON_PNG)
        .expect("decode tray icon")
        .to_rgba8();
    let (w, h) = img.dimensions();
    let icon = Icon::from_rgba(img.into_raw(), w, h).expect("build tray icon");

    let menu = Menu::new();
    // The three toggles start enabled. muda flips the checkmark itself on click;
    // the app keeps a matching flag in lockstep (see UserEvent::Toggle*), so we
    // don't need to hold on to these item handles.
    let action = CheckMenuItem::new("Send prompt on crack", true, true, None);
    let sound = CheckMenuItem::new("Play sound on crack", true, true, None);
    let roar = CheckMenuItem::new("Guanzhang RRRRR roar", true, true, None);

    // Skin submenu — one radio-style item per skin, the current one checked.
    let skin_menu = Submenu::new("Skin", true);
    let mut skin_items: Vec<CheckMenuItem> = Vec::with_capacity(skins.len());
    let mut skin_map: Vec<(MenuId, usize)> = Vec::with_capacity(skins.len());
    for (i, s) in skins.iter().enumerate() {
        let item = CheckMenuItem::new(s.label, true, i == current, None);
        skin_menu.append(&item).expect("append skin item");
        skin_map.push((item.id().clone(), i));
        skin_items.push(item);
    }

    let about = MenuItem::new("About agent-whip", true, None);
    let check_update = MenuItem::new("Check for Update", true, None);
    let quit = MenuItem::new("Quit", true, None);

    menu.append(&action).expect("append action item");
    menu.append(&sound).expect("append sound item");
    menu.append(&roar).expect("append roar item");
    menu.append(&skin_menu).expect("append skin submenu");
    menu.append(&PredefinedMenuItem::separator())
        .expect("append separator");
    menu.append(&about).expect("append about item");
    menu.append(&check_update).expect("append update item");
    menu.append(&quit).expect("append quit item");

    let action_id = action.id().clone();
    let sound_id = sound.id().clone();
    let roar_id = roar.id().clone();
    let about_id = about.id().clone();
    let check_update_id = check_update.id().clone();
    let quit_id = quit.id().clone();

    // Forward menu selections.
    {
        let proxy = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let ev = if e.id == action_id {
                UserEvent::ToggleAction
            } else if e.id == sound_id {
                UserEvent::ToggleSound
            } else if e.id == roar_id {
                UserEvent::ToggleRoar
            } else if e.id == about_id {
                UserEvent::ShowAbout
            } else if e.id == check_update_id {
                UserEvent::CheckUpdate
            } else if e.id == quit_id {
                UserEvent::Quit
            } else if let Some((_, idx)) = skin_map.iter().find(|(id, _)| *id == e.id) {
                UserEvent::SetSkin(*idx)
            } else {
                return;
            };
            let _ = proxy.send_event(ev);
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
    Tray {
        _tray: tray,
        skin_items,
    }
}
