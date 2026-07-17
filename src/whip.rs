//! Whip rope simulation — a faithful port of the Verlet physics in OpenWhip's
//! `overlay.html`. All tunables live in [`Params`]; the integration, distance
//! constraints, bend limits, wall slaps and crack detection mirror the original
//! `update()` step exactly.

use std::time::{Duration, Instant};

/// Physics settings — one-to-one with the `P` object in the original overlay.
#[derive(Clone, Copy)]
pub struct Params {
    // Rope structure
    pub segments: usize,
    pub segment_length: f32,
    pub taper: f32,

    // Physics
    pub gravity: f32,
    pub drop_gravity: f32,
    pub damping: f32,
    pub constraint_iters: usize,
    pub max_stretch_ratio: f32,

    // Dynamic handle aim
    pub base_target_angle: f32,
    pub handle_aim_by_mouse_x: f32,
    pub handle_aim_by_mouse_y: f32,
    pub handle_aim_clamp: f32,
    pub handle_spring: f32,
    pub handle_angular_damping: f32,
    pub base_pose_segments: usize,
    pub base_pose_stiff_start: f32,
    pub base_pose_stiff_end: f32,

    // Elastic bend limits
    pub handle_max_bend_deg: f32,
    pub tip_max_bend_deg: f32,
    pub bend_rigidity_start: f32,
    pub bend_rigidity_end: f32,

    // Screen-edge slap
    pub wall_bounce: f32,
    pub wall_friction: f32,

    // Crack detection
    pub crack_speed: f32,
    pub crack_cooldown: Duration,
    pub first_crack_grace: Duration,

    // Initial arc shape
    pub arc_width: f32,
    pub arc_height: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            segments: 28,
            segment_length: 25.0,
            taper: 0.6,

            gravity: 1.2,
            drop_gravity: 0.95,
            damping: 0.96,
            constraint_iters: 20,
            max_stretch_ratio: 1.2,

            base_target_angle: -1.12,
            handle_aim_by_mouse_x: 0.4,
            handle_aim_by_mouse_y: 0.2,
            handle_aim_clamp: 2.0,
            handle_spring: 0.7,
            handle_angular_damping: 0.078,
            base_pose_segments: 2,
            base_pose_stiff_start: 0.9,
            base_pose_stiff_end: 0.8,

            handle_max_bend_deg: 16.0,
            tip_max_bend_deg: 130.0,
            bend_rigidity_start: 0.8,
            bend_rigidity_end: 0.12,

            wall_bounce: 0.42,
            wall_friction: 0.86,

            crack_speed: 340.0,
            crack_cooldown: Duration::from_millis(200),
            first_crack_grace: Duration::from_millis(350),

            arc_width: 260.0,
            arc_height: 185.0,
        }
    }
}

/// A single chain link. `p*` are the previous positions used by Verlet.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub px: f32,
    pub py: f32,
}

/// What a single physics step produced, for the caller to act on.
#[derive(Default, Clone, Copy)]
pub struct StepOutcome {
    /// Tip broke the sound barrier this frame — play a sound + send the macro.
    pub crack: bool,
    /// The dropped whip has fully fallen off-screen — hide the overlay.
    pub finished: bool,
}

/// The rectangle the whip is confined to — the bounding box of all monitors in
/// sim space. Coordinates can be negative (monitors left of / above the
/// primary).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        }
    }
}

/// The whole simulation, mirroring the module-level state in `overlay.html`.
pub struct Sim {
    pub p: Params,
    pub pts: Vec<Point>,
    pub active: bool,
    pub dropping: bool,
    pub mouse: (f32, f32),
    pub prev_mouse: (f32, f32),
    handle_angle: f32,
    handle_ang_vel: f32,
    spawn_time: Instant,
    last_crack: Option<Instant>,
    /// No crack may fire before this time — set when the cursor crosses
    /// between monitors, where the naturally fast motion would false-trigger.
    crack_inhibit_until: Option<Instant>,
    pub bounds: Bounds,
}

impl Sim {
    pub fn new(p: Params) -> Self {
        Sim {
            p,
            pts: Vec::new(),
            active: false,
            dropping: false,
            mouse: (0.0, 0.0),
            prev_mouse: (0.0, 0.0),
            handle_angle: p.base_target_angle,
            handle_ang_vel: 0.0,
            spawn_time: Instant::now(),
            last_crack: None,
            crack_inhibit_until: None,
            bounds: Bounds::default(),
        }
    }

    /// Single-screen convenience: bounds spanning `(0,0)..(w,h)`.
    pub fn resize(&mut self, w: f32, h: f32) {
        self.bounds = Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: w,
            max_y: h,
        };
    }

    pub fn set_bounds(&mut self, bounds: Bounds) {
        self.bounds = bounds;
    }

    pub fn set_mouse(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
    }

    /// Suppress crack detection until `until`. The whip keeps animating; only
    /// the crack event (sound + keystrokes) is held back.
    pub fn inhibit_crack(&mut self, until: Instant) {
        self.crack_inhibit_until = Some(until);
    }

    /// Nice upward arc from the handle (mouse) to the tip.
    pub fn spawn(&mut self, mx: f32, my: f32, now: Instant) {
        let p = self.p;
        let mut pts = Vec::with_capacity(p.segments);
        for i in 0..p.segments {
            let t = i as f32 / (p.segments - 1) as f32;
            let x = mx + t * p.arc_width;
            let y = my - (t * std::f32::consts::PI * 0.75).sin() * p.arc_height;
            pts.push(Point { x, y, px: x, py: y });
        }
        self.pts = pts;
        self.active = true;
        self.dropping = false;
        self.last_crack = None;
        self.crack_inhibit_until = None;
        self.spawn_time = now;
        self.mouse = (mx, my);
        self.prev_mouse = (mx, my);
        self.handle_angle = p.base_target_angle;
        self.handle_ang_vel = 0.0;
    }

    /// Begin dropping the whip (equivalent to `drop-whip`).
    pub fn drop(&mut self) {
        if self.active && !self.dropping {
            self.dropping = true;
        }
    }

    /// One physics step. `now` is the frame time.
    pub fn step(&mut self, now: Instant) -> StepOutcome {
        let mut out = StepOutcome::default();
        if !self.active {
            return out;
        }

        let p = self.p;
        let g = if self.dropping {
            p.drop_gravity
        } else {
            p.gravity
        };

        update_handle_aim(
            &mut self.handle_angle,
            &mut self.handle_ang_vel,
            self.mouse,
            self.prev_mouse,
            &p,
            self.dropping,
        );

        let n = self.pts.len();
        let start = if self.dropping { 0 } else { 1 };

        // Verlet integration.
        for i in start..n {
            let pt = &mut self.pts[i];
            let vx = (pt.x - pt.px) * p.damping;
            let vy = (pt.y - pt.py) * p.damping;
            pt.px = pt.x;
            pt.py = pt.y;
            pt.x += vx;
            pt.y += vy + g;
        }

        // Pin handle to mouse.
        if !self.dropping {
            let (mx, my) = self.mouse;
            self.pts[0] = Point {
                x: mx,
                y: my,
                px: mx,
                py: my,
            };
        }

        cap_segment_stretch(&mut self.pts, &p);
        apply_wall_collisions(&mut self.pts, &p, self.bounds, self.dropping);
        apply_base_pose(&mut self.pts, &p, self.handle_angle, self.dropping);

        // Distance constraints (multiple iterations for stiffness).
        for _ in 0..p.constraint_iters {
            for i in 0..n - 1 {
                let a = self.pts[i];
                let b = self.pts[i + 1];
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let dist = (dx * dx + dy * dy).sqrt().max(0.0001);
                let target = seg_len(&p, i);
                let diff = (dist - target) / dist * 0.5;
                let ox = dx * diff;
                let oy = dy * diff;
                if i == 0 && !self.dropping {
                    // Handle is pinned — push only the next point.
                    self.pts[i + 1].x -= ox * 2.0;
                    self.pts[i + 1].y -= oy * 2.0;
                } else {
                    self.pts[i].x += ox;
                    self.pts[i].y += oy;
                    self.pts[i + 1].x -= ox;
                    self.pts[i + 1].y -= oy;
                }
            }
            apply_bend_limits(&mut self.pts, &p);
            if !self.dropping {
                apply_base_pose(&mut self.pts, &p, self.handle_angle, self.dropping);
            }
            cap_segment_stretch(&mut self.pts, &p);
            apply_wall_collisions(&mut self.pts, &p, self.bounds, self.dropping);
        }

        // Tip velocity for crack detection.
        let tip = self.pts[n - 1];
        let tip_vel = ((tip.x - tip.px).powi(2) + (tip.y - tip.py).powi(2)).sqrt();

        if !self.dropping && tip_vel > p.crack_speed {
            let past_grace = now.duration_since(self.spawn_time) >= p.first_crack_grace;
            let past_cooldown = self
                .last_crack
                .map(|t| now.duration_since(t) > p.crack_cooldown)
                .unwrap_or(true);
            let inhibited = self.crack_inhibit_until.map(|t| now < t).unwrap_or(false);
            if past_grace && past_cooldown && !inhibited {
                self.last_crack = Some(now);
                out.crack = true;
            }
        }

        // If dropping, check if everything fell off screen.
        if self.dropping && self.pts.iter().all(|pt| pt.y > self.bounds.max_y + 60.0) {
            self.active = false;
            self.dropping = false;
            out.finished = true;
        }

        self.prev_mouse = self.mouse;
        out
    }
}

#[inline]
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn wrap_pi(mut a: f32) -> f32 {
    use std::f32::consts::PI;
    while a > PI {
        a -= PI * 2.0;
    }
    while a < -PI {
        a += PI * 2.0;
    }
    a
}

pub fn seg_len(p: &Params, i: usize) -> f32 {
    let t = i as f32 / (p.segments - 1) as f32;
    p.segment_length * (1.0 - t * (1.0 - p.taper))
}

fn update_handle_aim(
    handle_angle: &mut f32,
    handle_ang_vel: &mut f32,
    mouse: (f32, f32),
    prev_mouse: (f32, f32),
    p: &Params,
    dropping: bool,
) {
    if dropping {
        return;
    }
    let mvx = mouse.0 - prev_mouse.0;
    let mvy = mouse.1 - prev_mouse.1;
    let delta = clamp(
        mvx * p.handle_aim_by_mouse_x + mvy * p.handle_aim_by_mouse_y,
        -p.handle_aim_clamp,
        p.handle_aim_clamp,
    );
    let target = p.base_target_angle + delta;
    let err = wrap_pi(target - *handle_angle);
    *handle_ang_vel += err * p.handle_spring;
    *handle_ang_vel *= p.handle_angular_damping;
    *handle_angle = wrap_pi(*handle_angle + *handle_ang_vel);
}

fn apply_base_pose(pts: &mut [Point], p: &Params, handle_angle: f32, dropping: bool) {
    if dropping || pts.is_empty() {
        return;
    }
    let dx = handle_angle.cos();
    let dy = handle_angle.sin();
    let guided = p.base_pose_segments.min(pts.len() - 1);
    for i in 1..=guided {
        let t = (i - 1) as f32 / (guided - 1).max(1) as f32;
        let stiff = lerp(p.base_pose_stiff_start, p.base_pose_stiff_end, t);
        let prev = pts[i - 1];
        let target_len = seg_len(p, i - 1);
        let tx = prev.x + dx * target_len;
        let ty = prev.y + dy * target_len;
        pts[i].x = lerp(pts[i].x, tx, stiff);
        pts[i].y = lerp(pts[i].y, ty, stiff);
    }
}

fn apply_bend_limits(pts: &mut [Point], p: &Params) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    for i in 1..n - 1 {
        let a = pts[i - 1];
        let b = pts[i];
        let c = pts[i + 1];

        let v1x = a.x - b.x;
        let v1y = a.y - b.y;
        let v2x = c.x - b.x;
        let v2y = c.y - b.y;
        let l1 = (v1x * v1x + v1y * v1y).sqrt().max(0.0001);
        let l2 = (v2x * v2x + v2y * v2y).sqrt().max(0.0001);
        let n1x = v1x / l1;
        let n1y = v1y / l1;
        let n2x = v2x / l2;
        let n2y = v2y / l2;

        let dot = clamp(n1x * n2x + n1y * n2y, -1.0, 1.0);
        let angle = dot.acos();
        let t = i as f32 / (n - 2) as f32;
        let max_bend =
            lerp(p.handle_max_bend_deg, p.tip_max_bend_deg, t) * std::f32::consts::PI / 180.0;
        let bend = std::f32::consts::PI - angle;
        if bend <= max_bend {
            continue;
        }

        let cross = n1x * n2y - n1y * n2x;
        let sign = if cross >= 0.0 { 1.0 } else { -1.0 };
        let target_angle = std::f32::consts::PI - max_bend;
        let target_a = n1y.atan2(n1x) + sign * target_angle;
        let tx = b.x + target_a.cos() * l2;
        let ty = b.y + target_a.sin() * l2;
        let rigidity = lerp(p.bend_rigidity_start, p.bend_rigidity_end, t);

        pts[i + 1].x = lerp(c.x, tx, rigidity);
        pts[i + 1].y = lerp(c.y, ty, rigidity);
    }
}

fn cap_segment_stretch(pts: &mut [Point], p: &Params) {
    if pts.len() < 2 {
        return;
    }
    for i in 0..pts.len() - 1 {
        let a = pts[i];
        let b = pts[i + 1];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = (dx * dx + dy * dy).sqrt().max(0.0001);
        let max_len = seg_len(p, i) * p.max_stretch_ratio;
        if dist <= max_len {
            continue;
        }
        let k = max_len / dist;
        pts[i + 1].x = a.x + dx * k;
        pts[i + 1].y = a.y + dy * k;
    }
}

fn apply_wall_collisions(pts: &mut [Point], p: &Params, b: Bounds, dropping: bool) {
    if dropping {
        return; // disable collisions while dropping
    }
    for i in 1..pts.len() {
        let pt = &mut pts[i];
        let mut vx = pt.x - pt.px;
        let mut vy = pt.y - pt.py;
        let mut hit = false;

        if pt.x < b.min_x {
            pt.x = b.min_x;
            if vx < 0.0 {
                vx = -vx * p.wall_bounce;
            }
            vy *= p.wall_friction;
            hit = true;
        } else if pt.x > b.max_x {
            pt.x = b.max_x;
            if vx > 0.0 {
                vx = -vx * p.wall_bounce;
            }
            vy *= p.wall_friction;
            hit = true;
        }

        if pt.y < b.min_y {
            pt.y = b.min_y;
            if vy < 0.0 {
                vy = -vy * p.wall_bounce;
            }
            vx *= p.wall_friction;
            hit = true;
        } else if pt.y > b.max_y {
            pt.y = b.max_y;
            if vy > 0.0 {
                vy = -vy * p.wall_bounce;
            }
            vx *= p.wall_friction;
            hit = true;
        }

        if hit {
            pt.px = pt.x - vx;
            pt.py = pt.y - vy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim() -> Sim {
        let mut s = Sim::new(Params::default());
        s.resize(1920.0, 1080.0);
        s
    }

    #[test]
    fn spawn_builds_full_chain() {
        let mut s = sim();
        s.spawn(500.0, 500.0, Instant::now());
        assert_eq!(s.pts.len(), Params::default().segments);
        // Handle sits at the mouse.
        assert_eq!(s.pts[0].x, 500.0);
        assert_eq!(s.pts[0].y, 500.0);
        // Tip is arc_width to the right of the handle.
        let tip = s.pts.last().unwrap();
        assert!((tip.x - (500.0 + 260.0)).abs() < 0.01);
    }

    #[test]
    fn handle_follows_mouse_each_step() {
        let mut s = sim();
        let t = Instant::now();
        s.spawn(500.0, 500.0, t);
        s.set_mouse(800.0, 300.0);
        s.step(t);
        assert_eq!(s.pts[0].x, 800.0);
        assert_eq!(s.pts[0].y, 300.0);
    }

    #[test]
    fn crack_grace_blocks_immediate_crack() {
        let mut s = sim();
        let t = Instant::now();
        s.spawn(500.0, 500.0, t);
        // Yank the mouse hard on the very first frame: tip is fast but within grace.
        s.set_mouse(1500.0, 500.0);
        let out = s.step(t);
        assert!(!out.crack, "no crack should fire within the grace window");
    }

    #[test]
    fn crack_fires_after_grace_on_fast_flick() {
        let mut s = sim();
        let t0 = Instant::now();
        s.spawn(500.0, 500.0, t0);
        let after = t0 + Duration::from_millis(400);
        // Settle a few frames so the chain has slack, then flick.
        for _ in 0..5 {
            s.step(after);
        }
        s.set_mouse(1600.0, 900.0);
        let mut cracked = false;
        for _ in 0..10 {
            if s.step(after).crack {
                cracked = true;
                break;
            }
        }
        assert!(cracked, "a hard flick past the grace window should crack");
    }

    #[test]
    fn inhibit_blocks_crack_until_window_expires() {
        let mut s = sim();
        let t0 = Instant::now();
        s.spawn(500.0, 500.0, t0);
        let after = t0 + Duration::from_millis(400);
        for _ in 0..5 {
            s.step(after);
        }
        // A monitor crossing was just detected: hold cracks for one second.
        s.inhibit_crack(after + Duration::from_secs(1));
        s.set_mouse(1600.0, 900.0);
        for _ in 0..10 {
            assert!(!s.step(after).crack, "no crack may fire while inhibited");
        }

        // Once the window has expired, the same hard flick cracks again.
        let later = after + Duration::from_secs(2);
        for _ in 0..5 {
            s.step(later);
        }
        s.set_mouse(300.0, 200.0);
        let mut cracked = false;
        for _ in 0..10 {
            if s.step(later).crack {
                cracked = true;
                break;
            }
        }
        assert!(cracked, "cracks must resume after the inhibit window");
    }

    #[test]
    fn negative_bounds_allow_crossing_into_left_monitor() {
        let mut s = Sim::new(Params::default());
        // Two side-by-side 1920x1080 monitors, secondary to the left of the
        // primary: the union spans x in [-1920, 1920].
        s.set_bounds(Bounds {
            min_x: -1920.0,
            min_y: 0.0,
            max_x: 1920.0,
            max_y: 1080.0,
        });
        let t = Instant::now();
        s.spawn(100.0, 500.0, t);
        // Drag the handle deep into the left monitor and settle.
        s.set_mouse(-1500.0, 500.0);
        for _ in 0..120 {
            s.step(t);
        }
        // The rope must follow past the old x=0 wall instead of piling up on it.
        assert!(
            s.pts.iter().any(|pt| pt.x < -100.0),
            "rope should cross into negative-x space, tip at {:?}",
            s.pts.last()
        );
        // And nothing may sit left of the union's outer wall.
        assert!(s.pts.iter().all(|pt| pt.x >= -1920.0));
    }

    #[test]
    fn dropped_whip_eventually_finishes() {
        let mut s = sim();
        let t = Instant::now();
        s.spawn(500.0, 500.0, t);
        s.drop();
        let mut finished = false;
        for _ in 0..2000 {
            if s.step(t).finished {
                finished = true;
                break;
            }
        }
        assert!(finished, "a dropped whip must fall off-screen and finish");
        assert!(!s.active);
    }
}
