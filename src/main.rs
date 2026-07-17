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
mod skins;
mod sound;
mod sparkle;
mod tray;
mod whip;

use logging::log;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

use render::RenderParams;
use whip::{Bounds, Params, Sim};

/// Events pushed into the loop from the tray (a different thread).
#[derive(Debug, Clone, Copy)]
pub enum UserEvent {
    TrayToggle,
    /// Toggle the crack "action" — the Ctrl-C + typed-phrase keystroke macro.
    ToggleAction,
    /// Toggle the crack sound.
    ToggleSound,
    /// Toggle the Guanzhang RRRRR roar layered over the crack.
    ToggleRoar,
    /// Pick a whip skin by index into `App::skins`.
    SetSkin(usize),
    /// Check GitHub for a newer release and offer to install it.
    CheckUpdate,
    /// Show a small dialog with the app version (tray "About" item).
    ShowAbout,
    Quit,
}

const FRAME: Duration = Duration::from_millis(16);
/// Ignore "drop" clicks for this long after spawning, so the spawning click
/// (e.g. releasing the tray) doesn't instantly drop the whip.
const SPAWN_GUARD: Duration = Duration::from_millis(200);
/// Extra slack (sim px) around a monitor when deciding whether the whip
/// touches it — covers glow blur, stroke width, and spline overshoot.
const DRAW_MARGIN: f32 = 100.0;
/// Hold cracks for this long after the cursor crosses between monitors —
/// throwing the cursor to another screen is naturally fast and would
/// false-trigger the crack (sound + typed command).
const MONITOR_CROSS_GRACE: Duration = Duration::from_millis(500);

/// One transparent click-through window covering one monitor. The sim runs in
/// a global space; each overlay maps it into its own pixels as
/// `p_win = p_sim * k - offset`.
struct Overlay {
    window: Arc<Window>,
    gpu: gpu::Gpu,
    pixmap: Pixmap,
    /// `monitor_scale / ref_scale` — 1.0 on same-DPI setups.
    k: f32,
    /// The monitor's origin in its own physical pixels (logical pos × scale).
    offset: (f32, f32),
    /// Stroke widths pre-scaled by `k`.
    rp: RenderParams,
    /// Whether the last presented frame had whip pixels, so we clear exactly
    /// once after the whip leaves this monitor instead of re-uploading forever.
    drawn_last_frame: bool,
}

/// What the overlays were built for; when this changes (monitor hot-plug,
/// rearrangement, resolution change) they are rebuilt on the next summon.
type MonitorSig = Vec<((i32, i32), (u32, u32), u64)>;

fn monitor_sig(el: &ActiveEventLoop) -> MonitorSig {
    el.available_monitors()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            ((p.x, p.y), (s.width, s.height), m.scale_factor().to_bits())
        })
        .collect()
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    dry_run: bool,
    gpu_ctx: gpu::GpuContext,
    overlays: Vec<Overlay>,
    monitors_sig: MonitorSig,
    sim: Sim,
    rp: RenderParams,
    input: Option<input::Input>,
    warned_no_perm: bool,
    sound: sound::Sound,
    tray: Option<tray::Tray>,
    visible: bool,
    /// The primary monitor's scale factor — global cursor points × this give
    /// sim coordinates (matching the old single-monitor behavior exactly).
    ref_scale: f64,
    /// Primary monitor's center in sim space (self-test cursor orbit).
    primary_center: (f32, f32),
    /// Monitor rects `(x, y, w, h)` in global logical points, in overlay order
    /// — used to notice the cursor crossing between monitors.
    monitor_rects: Vec<(f64, f64, f64, f64)>,
    /// Which of `monitor_rects` the cursor was on last frame.
    cursor_monitor: Option<usize>,
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
    /// Whether a crack fires the keystroke macro (toggled from the tray menu).
    action_enabled: bool,
    /// Whether a crack plays a sound (toggled from the tray menu).
    sound_enabled: bool,
    /// Whether the Guanzhang RRRRR roar is layered over the crack (tray toggle).
    roar_enabled: bool,
    /// Available whip skins and the index of the selected one (tray "Skin"
    /// submenu; the choice is persisted across restarts).
    skins: Vec<skins::Skin>,
    skin_idx: usize,
    /// Sparkle auto-updater; `None` if the framework isn't embedded.
    updater: Option<sparkle::Updater>,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>, dry_run: bool, selftest: bool) -> Self {
        let skins = skins::all();
        let skin_idx = skins::index_of(&skins::load_selected_id());
        App {
            proxy,
            dry_run,
            gpu_ctx: gpu::GpuContext::new(),
            overlays: Vec::new(),
            monitors_sig: Vec::new(),
            sim: Sim::new(Params::default()),
            rp: RenderParams::default(),
            input: None,
            warned_no_perm: false,
            sound: sound::Sound::new(),
            tray: None,
            visible: false,
            ref_scale: 1.0,
            primary_center: (640.0, 400.0),
            monitor_rects: Vec::new(),
            cursor_monitor: None,
            spawn_at: None,
            cfg: config::ConfigFile::init(),
            enigo: None,
            pending_type: None,
            selftest,
            tick: 0,
            action_enabled: true,
            sound_enabled: true,
            roar_enabled: true,
            skins,
            skin_idx,
            updater: None,
        }
    }

    /// Create (or rebuild, when the monitor layout changed) one overlay window
    /// per monitor, and size the sim's wall bounds to the union of all of them.
    ///
    /// The sim runs in "reference pixels": global logical points × the primary
    /// monitor's scale factor. On a single monitor this matches the old
    /// single-window behavior exactly.
    fn ensure_overlays(&mut self, el: &ActiveEventLoop) {
        let sig = monitor_sig(el);
        if !self.overlays.is_empty() && sig == self.monitors_sig {
            return;
        }
        if !self.overlays.is_empty() {
            log!("agent-whip: monitor layout changed, rebuilding overlays");
        }
        self.overlays.clear();
        self.monitor_rects.clear();
        self.cursor_monitor = None;
        self.monitors_sig = sig;

        self.ref_scale = el
            .primary_monitor()
            .or_else(|| el.available_monitors().next())
            .map(|m| m.scale_factor())
            .unwrap_or(1.0);
        let primary_pos = el.primary_monitor().map(|m| m.position());

        /// One monitor's geometry in global logical points (the cursor's space).
        struct Mon {
            pos: (f64, f64),
            size: (f64, f64),
            scale: f64,
        }
        // winit reports position/size in each monitor's own physical pixels, so
        // divide its scale back out.
        let mut monitors: Vec<Mon> = el
            .available_monitors()
            .map(|m| {
                let scale = m.scale_factor();
                let p = m.position();
                let s = m.size();
                Mon {
                    pos: (p.x as f64 / scale, p.y as f64 / scale),
                    size: (s.width as f64 / scale, s.height as f64 / scale),
                    scale,
                }
            })
            .collect();
        if monitors.is_empty() {
            monitors.push(Mon {
                pos: (0.0, 0.0),
                size: (1280.0, 800.0),
                scale: 1.0,
            });
        }

        let mut bounds = Bounds {
            min_x: f32::MAX,
            min_y: f32::MAX,
            max_x: f32::MIN,
            max_y: f32::MIN,
        };
        for (idx, &Mon { pos, size, scale }) in monitors.iter().enumerate() {
            self.monitor_rects.push((pos.0, pos.1, size.0, size.1));
            bounds.min_x = bounds.min_x.min((pos.0 * self.ref_scale) as f32);
            bounds.min_y = bounds.min_y.min((pos.1 * self.ref_scale) as f32);
            bounds.max_x = bounds.max_x.max(((pos.0 + size.0) * self.ref_scale) as f32);
            bounds.max_y = bounds.max_y.max(((pos.1 + size.1) * self.ref_scale) as f32);

            // Logical position/size sidestep any physical↔logical conversion
            // ambiguity while the window is still off its target monitor.
            let attrs = Window::default_attributes()
                .with_title(format!("agent-whip {idx}"))
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_visible(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_inner_size(LogicalSize::new(size.0, size.1))
                .with_position(LogicalPosition::new(pos.0, pos.1));
            let window = Arc::new(el.create_window(attrs).expect("create overlay window"));
            // Click-through: pointer events pass to the app underneath. We read
            // the cursor globally instead (see input.rs).
            let _ = window.set_cursor_hittest(false);

            let gpu = gpu::Gpu::new(&mut self.gpu_ctx, window.clone());
            let (w, h) = gpu.size();
            let pixmap = Pixmap::new(w, h).expect("allocate overlay pixmap");
            let k = (scale / self.ref_scale) as f32;
            self.overlays.push(Overlay {
                window,
                gpu,
                pixmap,
                k,
                offset: ((pos.0 * scale) as f32, (pos.1 * scale) as f32),
                rp: self.rp.scaled(k),
                drawn_last_frame: false,
            });

            let is_primary = match primary_pos {
                Some(pp) => {
                    (pp.x as f64 - pos.0 * scale).abs() < 1.0
                        && (pp.y as f64 - pos.1 * scale).abs() < 1.0
                }
                None => idx == 0,
            };
            if is_primary {
                self.primary_center = (
                    ((pos.0 + size.0 * 0.5) * self.ref_scale) as f32,
                    ((pos.1 + size.1 * 0.5) * self.ref_scale) as f32,
                );
            }
        }
        self.sim.set_bounds(bounds);
        log!(
            "agent-whip: {} overlay(s), sim bounds ({:.0},{:.0})..({:.0},{:.0})",
            self.overlays.len(),
            bounds.min_x,
            bounds.min_y,
            bounds.max_x,
            bounds.max_y
        );
    }

    /// Map a global cursor position (logical points, primary-display origin) to
    /// sim coordinates.
    fn map_cursor(&self, c: (i32, i32)) -> (f32, f32) {
        (
            (c.0 as f64 * self.ref_scale) as f32,
            (c.1 as f64 * self.ref_scale) as f32,
        )
    }

    /// Which monitor a global cursor position (logical points) is on.
    fn monitor_at(&self, c: (i32, i32)) -> Option<usize> {
        let (x, y) = (c.0 as f64, c.1 as f64);
        self.monitor_rects
            .iter()
            .position(|&(mx, my, mw, mh)| x >= mx && x < mx + mw && y >= my && y < my + mh)
    }

    /// Note which monitor the cursor is on; on a crossing, hold cracks briefly
    /// so the naturally fast throw to another screen doesn't false-trigger.
    fn track_cursor_monitor(&mut self, cursor: (i32, i32), now: Instant) {
        let cur = self.monitor_at(cursor);
        if let (Some(prev), Some(new)) = (self.cursor_monitor, cur)
            && prev != new
        {
            self.sim.inhibit_crack(now + MONITOR_CROSS_GRACE);
        }
        if cur.is_some() {
            self.cursor_monitor = cur;
        }
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
        // Whip is up → drop it.
        if self.visible && self.sim.active && !self.sim.dropping {
            self.sim.drop();
            return;
        }
        // Spawn a fresh whip at the cursor.
        if !self.visible {
            // Rebuilds the overlays if monitors were (un)plugged or rearranged.
            self.ensure_overlays(el);
            if !self.input_ready() {
                return;
            }
            let cursor = {
                let input = self.input.as_mut().unwrap();
                input.sync_button();
                input.cursor()
            };
            let (mx, my) = self.map_cursor(cursor);
            // Seed the crossing tracker so the first poll isn't a "crossing".
            self.cursor_monitor = self.monitor_at(cursor);
            let now = Instant::now();
            self.sim.spawn(mx, my, now);
            self.spawn_at = Some(now);
            self.visible = true;
            for ov in &self.overlays {
                ov.window.set_visible(true);
                ov.window.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
        }
    }

    fn hide(&mut self, el: &ActiveEventLoop) {
        for ov in &self.overlays {
            ov.window.set_visible(false);
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

        if self.sound_enabled {
            let sound = self.cfg.pick_sound();
            self.sound.play_crack(sound, self.roar_enabled);
        }

        // The keystroke macro is the "action"; the tray menu can switch it off.
        if !self.action_enabled {
            if self.dry_run {
                log!("[dry-run] crack -> action off (no keystrokes)");
            }
            return;
        }

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

    /// Render one overlay: transform the sim points into its window space and
    /// draw, skipping the rasterization entirely while the whip is off that
    /// monitor (clearing once as it leaves).
    fn render_overlay(&mut self, idx: usize) {
        if !self.visible || idx >= self.overlays.len() {
            return;
        }
        let skin = self.skins[self.skin_idx];
        let active = self.sim.active && self.sim.pts.len() >= 2;
        let ov = &mut self.overlays[idx];

        let pts: Vec<whip::Point> = self
            .sim
            .pts
            .iter()
            .map(|p| whip::Point {
                x: p.x * ov.k - ov.offset.0,
                y: p.y * ov.k - ov.offset.1,
                px: p.px * ov.k - ov.offset.0,
                py: p.py * ov.k - ov.offset.1,
            })
            .collect();

        let (w, h) = ov.gpu.size();
        let margin = DRAW_MARGIN * ov.k;
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for p in &pts {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        let touches = active
            && max_x >= -margin
            && min_x <= w as f32 + margin
            && max_y >= -margin
            && min_y <= h as f32 + margin;

        if touches {
            render::draw(&mut ov.pixmap, &pts, &ov.rp, &skin);
            ov.gpu.render(ov.pixmap.data());
            ov.drawn_last_frame = true;
        } else if ov.drawn_last_frame {
            ov.pixmap.fill(tiny_skia::Color::TRANSPARENT);
            ov.gpu.render(ov.pixmap.data());
            ov.drawn_last_frame = false;
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.ensure_overlays(el);
        if self.tray.is_none() {
            self.tray = Some(tray::build(self.proxy.clone(), &self.skins, self.skin_idx));
            // Start Sparkle and do one silent background check on launch (per
            // Terry's request). Sparkle also runs its own scheduled checks.
            if !self.selftest {
                self.updater = sparkle::Updater::start();
                if let Some(u) = &self.updater {
                    u.check_in_background();
                }
            }
        }
        if self.selftest {
            let (cx, cy) = self.primary_center;
            let now = Instant::now();
            self.sim.spawn(cx, cy, now);
            self.spawn_at = Some(now);
            self.visible = true;
            for ov in &self.overlays {
                ov.window.set_visible(true);
                ov.window.request_redraw();
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
            // The tray checkmark already flipped (muda toggles it on click); keep
            // our flag in lockstep with it.
            UserEvent::ToggleAction => {
                self.action_enabled = !self.action_enabled;
                log!("agent-whip: action {}", on_off(self.action_enabled));
            }
            UserEvent::ToggleSound => {
                self.sound_enabled = !self.sound_enabled;
                log!("agent-whip: sound {}", on_off(self.sound_enabled));
            }
            UserEvent::ToggleRoar => {
                self.roar_enabled = !self.roar_enabled;
                log!("agent-whip: RRRRR roar {}", on_off(self.roar_enabled));
            }
            UserEvent::SetSkin(idx) => {
                if idx < self.skins.len() {
                    self.skin_idx = idx;
                    let id = self.skins[idx].id;
                    skins::save_selected_id(id);
                    if let Some(t) = &self.tray {
                        t.select_skin(idx);
                    }
                    log!("agent-whip: skin -> {id}");
                    if self.visible {
                        for ov in &self.overlays {
                            ov.window.request_redraw();
                        }
                    }
                }
            }
            UserEvent::CheckUpdate => {
                if let Some(u) = &self.updater {
                    u.check_for_updates();
                }
            }
            UserEvent::ShowAbout => show_about(),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let idx = self.overlays.iter().position(|ov| ov.window.id() == id);
        match event {
            // Keep the app alive in the tray; never actually close.
            WindowEvent::CloseRequested => self.hide(el),
            WindowEvent::Resized(size) => {
                if let Some(idx) = idx {
                    let ov = &mut self.overlays[idx];
                    ov.gpu.resize(size.width, size.height);
                    let (w, h) = ov.gpu.size();
                    ov.pixmap = Pixmap::new(w, h).expect("allocate overlay pixmap");
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(idx) = idx {
                    self.render_overlay(idx);
                }
            }
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
            let (cx, cy) = self.primary_center;
            let t = self.tick as f32;
            let mx = cx + 500.0 * (t * 0.25).sin();
            let my = cy + 250.0 * (t * 0.17).cos();
            self.sim.set_mouse(mx, my);
            let out = self.sim.step(now);
            if out.crack {
                self.crack(now);
            }
            for ov in &self.overlays {
                ov.window.request_redraw();
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
        self.track_cursor_monitor(cursor, now);
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

        for ov in &self.overlays {
            ov.window.request_redraw();
        }
        el.set_control_flow(ControlFlow::WaitUntil(now + FRAME));
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// Pop a small native dialog showing the version (tray "About" item). Runs the
/// dialog on a background thread so the event loop keeps ticking while it's open.
fn show_about() {
    let msg = format!("agent-whip v{}", env!("CARGO_PKG_VERSION"));
    log!("{msg}");
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        let script = format!(
            "display dialog \"{msg}\" with title \"agent-whip\" \
             buttons {{\"OK\"}} default button \"OK\""
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status();
    });
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

/// Render a static whip pose in the named skin to a PNG, composited over a dark
/// background so black-leather skins are visible. A dev/preview tool, not part
/// of normal use. Returns a process exit code.
fn render_skin_to_png(id: &str, out: &str) -> i32 {
    let skins = skins::all();
    let skin = skins[skins::index_of(id)];
    let (w, h) = (620u32, 380u32);

    let mut sim = Sim::new(Params::default());
    sim.resize(w as f32, h as f32);
    sim.spawn(130.0, h as f32 * 0.74, Instant::now());

    let (Some(mut whip_pm), Some(mut pm)) = (Pixmap::new(w, h), Pixmap::new(w, h)) else {
        eprintln!("render-skin: could not allocate pixmap");
        return 1;
    };
    render::draw(&mut whip_pm, &sim.pts, &RenderParams::default(), &skin);
    pm.fill(tiny_skia::Color::from_rgba8(18, 10, 10, 255));
    pm.draw_pixmap(
        0,
        0,
        whip_pm.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    match pm.save_png(out) {
        Ok(()) => {
            println!("wrote {out} ({})", skin.id);
            0
        }
        Err(e) => {
            eprintln!("render-skin: {e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // `agent-whip --version` prints the version and exits.
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("agent-whip {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Subcommand: `agent-whip whip` toggles a running instance, then exits.
    #[cfg(unix)]
    if args.iter().any(|a| a == "whip") {
        std::process::exit(run_whip_command());
    }

    // Dev/preview: `agent-whip render-skin <id> <out.png>` renders a static whip
    // pose in the given skin to a PNG (no window, no tray), then exits.
    if let Some(pos) = args.iter().position(|a| a == "render-skin") {
        let id = args.get(pos + 1).map(String::as_str).unwrap_or("notorious");
        let out = args.get(pos + 2).map(String::as_str).unwrap_or("skin.png");
        std::process::exit(render_skin_to_png(id, out));
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
