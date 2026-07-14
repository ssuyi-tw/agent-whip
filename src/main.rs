//! agent-whip — a native Rust port of OpenWhip.
//!
//! Sits in the tray. Click the tray icon to spawn a whip that follows your
//! cursor as a transparent, always-on-top overlay; flick it fast and the tip
//! cracks — playing a sound and typing a phrase into whatever app you were
//! using (by default a non-interrupting `/btw` nudge; Ctrl-C is opt-in via
//! config). Click (or click the tray again) to drop it.

mod config;
mod gpu;
mod input;
mod keys;
mod logging;
mod render;
mod sound;
mod tray;
mod whip;

use logging::log;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

use render::RenderParams;
use whip::{Params, Sim};

/// Events pushed into the loop from the tray (a different thread).
#[derive(Debug, Clone, Copy)]
pub enum UserEvent {
    TrayToggle,
    Quit,
}

const FRAME: Duration = Duration::from_millis(16);
/// Ignore "drop" clicks for this long after spawning, so the spawning click
/// (e.g. releasing the tray) doesn't instantly drop the whip.
const SPAWN_GUARD: Duration = Duration::from_millis(200);

struct App {
    proxy: EventLoopProxy<UserEvent>,
    dry_run: bool,
    window: Option<Arc<Window>>,
    gpu: Option<gpu::Gpu>,
    pixmap: Option<Pixmap>,
    sim: Sim,
    rp: RenderParams,
    input: Option<input::Input>,
    warned_no_perm: bool,
    sound: sound::Sound,
    tray: Option<tray::Tray>,
    visible: bool,
    scale: f64,
    monitor_pos: (i32, i32),
    spawn_at: Option<Instant>,
    /// User-editable crack config (phrases + toggles), reloaded on change.
    cfg: config::ConfigFile,
    /// Keystroke backend — created lazily on the main thread on first crack.
    enigo: Option<enigo::Enigo>,
    /// Deferred second half of the crack macro: type this phrase (+ whether to
    /// press Enter) at this time — the 300 ms gap after the interrupt.
    pending_type: Option<(Instant, String, bool)>,
    /// Debug mode: auto-spawn and animate with a synthetic cursor, then exit.
    selftest: bool,
    tick: u64,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>, dry_run: bool, selftest: bool) -> Self {
        App {
            proxy,
            dry_run,
            window: None,
            gpu: None,
            pixmap: None,
            sim: Sim::new(Params::default()),
            rp: RenderParams::default(),
            input: None,
            warned_no_perm: false,
            sound: sound::Sound::new(),
            tray: None,
            visible: false,
            scale: 1.0,
            monitor_pos: (0, 0),
            spawn_at: None,
            cfg: config::ConfigFile::init(),
            enigo: None,
            pending_type: None,
            selftest,
            tick: 0,
        }
    }

    fn ensure_window(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let monitor = el.primary_monitor().or_else(|| el.available_monitors().next());
        let (pos, size) = match &monitor {
            Some(m) => (m.position(), m.size()),
            None => (PhysicalPosition::new(0, 0), PhysicalSize::new(1280, 800)),
        };

        let attrs = Window::default_attributes()
            .with_title("agent-whip")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(size)
            .with_position(pos);
        let window = Arc::new(el.create_window(attrs).expect("create overlay window"));
        // Click-through: pointer events pass to the app underneath. We read the
        // cursor globally instead (see input.rs).
        let _ = window.set_cursor_hittest(false);

        self.scale = window.scale_factor();
        self.monitor_pos = (pos.x, pos.y);

        let g = gpu::Gpu::new(window.clone());
        let (w, h) = g.size();
        self.sim.resize(w as f32, h as f32);
        self.pixmap = Pixmap::new(w, h);
        self.gpu = Some(g);
        self.window = Some(window);
    }

    /// Map a global cursor position (logical points, primary-display origin) to
    /// the overlay surface's physical pixels.
    fn map_cursor(&self, c: (i32, i32)) -> (f32, f32) {
        let x = c.0 as f64 * self.scale - self.monitor_pos.0 as f64;
        let y = c.1 as f64 * self.scale - self.monitor_pos.1 as f64;
        (x as f32, y as f32)
    }

    /// Lazily bring up global input access (prompts for macOS Accessibility on
    /// first use). Returns whether input is available.
    fn input_ready(&mut self) -> bool {
        if self.input.is_none() {
            self.input = input::Input::try_new();
            if self.input.is_none() && !self.warned_no_perm {
                self.warned_no_perm = true;
                log!(
                    "agent-whip needs Accessibility permission to follow the cursor and \
                     send keystrokes. Grant it in System Settings ▸ Privacy & Security ▸ \
                     Accessibility (a prompt may have appeared), then relaunch agent-whip."
                );
            }
        }
        self.input.is_some()
    }

    fn toggle(&mut self, el: &ActiveEventLoop) {
        self.ensure_window(el);

        // Whip is up → drop it.
        if self.visible && self.sim.active && !self.sim.dropping {
            self.sim.drop();
            return;
        }
        // Spawn a fresh whip at the cursor.
        if !self.visible {
            if !self.input_ready() {
                return;
            }
            let cursor = {
                let input = self.input.as_mut().unwrap();
                input.sync_button();
                input.cursor()
            };
            let (mx, my) = self.map_cursor(cursor);
            let now = Instant::now();
            self.sim.spawn(mx, my, now);
            self.spawn_at = Some(now);
            self.visible = true;
            if let Some(w) = &self.window {
                w.set_visible(true);
                w.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
        }
    }

    fn hide(&mut self, el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.set_visible(false);
        }
        self.visible = false;
        // Stay ticking if a deferred keystroke is still owed.
        if self.pending_type.is_some() {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME));
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }

    /// Handle a crack: play a sound and send the interrupt now, scheduling the
    /// typed phrase for 300 ms later. All keystroke work stays on this (main)
    /// thread — enigo's macOS backend asserts it must.
    fn crack(&mut self, now: Instant) {
        self.cfg.reload_if_changed();
        let sound = self.cfg.pick_sound();
        self.sound.play_crack(sound);
        let send_interrupt = self.cfg.send_interrupt();
        let send_enter = self.cfg.send_enter();
        let phrase = self.cfg.pick_phrase();

        if self.dry_run {
            log!(
                "[dry-run] crack -> {}type {:?}{}",
                if send_interrupt { "Ctrl-C + " } else { "" },
                phrase,
                if send_enter { " + Enter" } else { "" }
            );
            return;
        }
        if self.enigo.is_none() {
            self.enigo = keys::new_enigo();
        }
        if let Some(e) = &mut self.enigo {
            if send_interrupt {
                keys::interrupt(e);
            }
            self.pending_type = Some((now + Duration::from_millis(300), phrase, send_enter));
        }
    }

    /// Fire the deferred typed phrase once its time arrives. Returns whether one
    /// is still pending (so the loop keeps ticking until it fires).
    fn service_pending(&mut self, now: Instant) -> bool {
        let due = matches!(&self.pending_type, Some((at, _, _)) if now >= *at);
        if due {
            if let Some((_, phrase, send_enter)) = self.pending_type.take() {
                if let Some(e) = &mut self.enigo {
                    keys::type_phrase(e, &phrase, send_enter);
                }
            }
        }
        self.pending_type.is_some()
    }

    fn render_frame(&mut self) {
        if !self.visible {
            return;
        }
        if let (Some(g), Some(pm)) = (&mut self.gpu, &mut self.pixmap) {
            render::draw(pm, &self.sim, &self.rp);
            g.render(pm.data());
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.ensure_window(el);
        if self.tray.is_none() {
            self.tray = Some(tray::build(self.proxy.clone()));
        }
        if self.selftest {
            let (cx, cy) = (self.sim.w * 0.5, self.sim.h * 0.5);
            let now = Instant::now();
            self.sim.spawn(cx, cy, now);
            self.spawn_at = Some(now);
            self.visible = true;
            if let Some(w) = &self.window {
                w.set_visible(true);
                w.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
            return;
        }
        el.set_control_flow(ControlFlow::Wait);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Quit => el.exit(),
            UserEvent::TrayToggle => self.toggle(el),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            // Keep the app alive in the tray; never actually close.
            WindowEvent::CloseRequested => self.hide(el),
            WindowEvent::Resized(size) => {
                if let Some(g) = &mut self.gpu {
                    g.resize(size.width, size.height);
                    let (w, h) = g.size();
                    self.sim.resize(w as f32, h as f32);
                    self.pixmap = Pixmap::new(w, h);
                }
            }
            WindowEvent::RedrawRequested => self.render_frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let pending = self.service_pending(now);

        if !self.visible {
            // Keep ticking only while a deferred keystroke is still owed.
            if pending {
                el.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
            } else {
                el.set_control_flow(ControlFlow::Wait);
            }
            return;
        }

        // Self-test: drive a synthetic cursor so the whip animates and cracks
        // without device_query (Accessibility) or a tray click. Exits at ~5s.
        if self.selftest {
            self.tick += 1;
            if self.tick > 300 {
                el.exit();
                return;
            }
            let cx = self.sim.w * 0.5;
            let cy = self.sim.h * 0.5;
            let t = self.tick as f32;
            let mx = cx + 500.0 * (t * 0.25).sin();
            let my = cy + 250.0 * (t * 0.17).cos();
            self.sim.set_mouse(mx, my);
            let out = self.sim.step(now);
            if out.crack {
                self.crack(now);
            }
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
            return;
        }

        let (cursor, left_edge) = match self.input.as_mut().map(|i| i.poll()) {
            Some(v) => v,
            None => {
                self.hide(el);
                return;
            }
        };
        let (mx, my) = self.map_cursor(cursor);
        self.sim.set_mouse(mx, my);

        // A fresh click drops the whip (after the spawn guard).
        if left_edge {
            let past_guard = self
                .spawn_at
                .map(|t| now.duration_since(t) > SPAWN_GUARD)
                .unwrap_or(true);
            if past_guard {
                self.sim.drop();
            }
        }

        let out = self.sim.step(now);
        if out.crack {
            self.crack(now);
        }
        if out.finished {
            self.hide(el);
            return;
        }

        if let Some(w) = &self.window {
            w.request_redraw();
        }
        el.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
    }
}

/// `agent-whip whip` — signal a running instance to summon (or drop) the whip.
/// This is what the CLI command and the Raycast script call.
#[cfg(unix)]
fn run_whip_command() -> i32 {
    let Some(pidfile) = config::pid_path() else {
        eprintln!("agent-whip: can't locate the pidfile");
        return 1;
    };
    let Ok(text) = std::fs::read_to_string(&pidfile) else {
        eprintln!("agent-whip isn't running — launch AgentWhip first.");
        return 1;
    };
    let pid = text.trim();
    let ok = std::process::Command::new("kill")
        .args(["-USR1", pid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        0
    } else {
        eprintln!("agent-whip: couldn't signal pid {pid} (not running? stale pidfile)");
        1
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Subcommand: `agent-whip whip` toggles a running instance, then exits.
    #[cfg(unix)]
    if args.iter().any(|a| a == "whip") {
        std::process::exit(run_whip_command());
    }

    let selftest = args.iter().any(|a| a == "--selftest");
    // Self-test never injects keystrokes.
    let dry_run = selftest || args.iter().any(|a| a == "--dry-run");
    log!(
        "agent-whip {} starting; logging to {}",
        env!("CARGO_PKG_VERSION"),
        logging::path().display()
    );
    if dry_run {
        log!("agent-whip: --dry-run (keystroke injection disabled)");
    }
    if selftest {
        log!("agent-whip: --selftest (synthetic cursor, will exit)");
    }

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        // No dock icon; behave like a menu-bar agent and don't steal focus.
        builder.with_activation_policy(ActivationPolicy::Accessory);
        builder.with_default_menu(false);
    }
    let event_loop = builder.build().expect("build event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy = event_loop.create_proxy();

    // Let external commands summon the whip: record our pid, and forward
    // SIGUSR1 into the event loop as a toggle (same as a tray click).
    #[cfg(unix)]
    {
        if let Some(p) = config::pid_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&p, std::process::id().to_string());
        }
        let proxy = proxy.clone();
        match signal_hook::iterator::Signals::new([signal_hook::consts::SIGUSR1]) {
            Ok(mut signals) => {
                std::thread::spawn(move || {
                    for _ in signals.forever() {
                        let _ = proxy.send_event(UserEvent::TrayToggle);
                    }
                });
            }
            Err(e) => log!("agent-whip: could not install signal handler: {e}"),
        }
    }

    let mut app = App::new(proxy, dry_run, selftest);
    event_loop.run_app(&mut app).expect("run event loop");

    #[cfg(unix)]
    if let Some(p) = config::pid_path() {
        let _ = std::fs::remove_file(p);
    }
}
