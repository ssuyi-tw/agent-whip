//! Whip rendering — ports `overlay.html`'s `draw()` to `tiny-skia`.
//!
//! The rope is drawn as a Catmull-Rom spline through the chain points, rendered
//! as cubic béziers. Two passes: a white halo (a fat stroke, extra-fat over the
//! handle links) then the dark core, tapering from handle to tip.

use crate::whip::{Point, Sim};
use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Visual-only tunables — one-to-one with the `Visuals` block of `P`.
#[derive(Clone, Copy)]
pub struct RenderParams {
    pub line_width_handle: f32,
    pub line_width_tip: f32,
    pub outline_width: f32,
    pub handle_extra_width: f32,
    pub handle_thick_segments: usize,
}

impl Default for RenderParams {
    fn default() -> Self {
        RenderParams {
            line_width_handle: 7.0,
            line_width_tip: 5.0,
            outline_width: 3.0,
            handle_extra_width: 5.0,
            handle_thick_segments: 2,
        }
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Point on the extrapolated Catmull-Rom control polygon for index `i`.
fn catmull_point(pts: &[Point], i: i32) -> (f32, f32) {
    let n = pts.len() as i32;
    if n == 0 {
        return (0.0, 0.0);
    }
    if i < 0 {
        if n >= 2 {
            return (2.0 * pts[0].x - pts[1].x, 2.0 * pts[0].y - pts[1].y);
        }
        return (pts[0].x, pts[0].y);
    }
    if i >= n {
        if n >= 2 {
            let a = pts[(n - 2) as usize];
            let b = pts[(n - 1) as usize];
            return (2.0 * b.x - a.x, 2.0 * b.y - a.y);
        }
        let last = pts[(n - 1) as usize];
        return (last.x, last.y);
    }
    let p = pts[i as usize];
    (p.x, p.y)
}

/// Cubic bézier (cp1, cp2, end) matching a uniform Catmull-Rom from `p1`→`p2`.
struct Bez {
    cp1x: f32,
    cp1y: f32,
    cp2x: f32,
    cp2y: f32,
    x2: f32,
    y2: f32,
}

fn whip_segment_bezier(pts: &[Point], i: usize) -> Bez {
    let ii = i as i32;
    let (p0x, p0y) = catmull_point(pts, ii - 1);
    let p1 = pts[i];
    let p2 = pts[i + 1];
    let (p3x, p3y) = catmull_point(pts, ii + 2);
    Bez {
        cp1x: p1.x + (p2.x - p0x) / 6.0,
        cp1y: p1.y + (p2.y - p0y) / 6.0,
        cp2x: p2.x - (p3x - p1.x) / 6.0,
        cp2y: p2.y - (p3y - p1.y) / 6.0,
        x2: p2.x,
        y2: p2.y,
    }
}

/// Clear the pixmap and, if a whip is present, draw it.
pub fn draw(pixmap: &mut Pixmap, sim: &Sim, rp: &RenderParams) {
    pixmap.fill(Color::TRANSPARENT);

    if !sim.active || sim.pts.len() < 2 {
        return;
    }
    let pts = &sim.pts;
    let n = pts.len();

    let stroke = |width: f32| Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    let mut white = Paint::default();
    white.anti_alias = true;
    white.set_color(Color::from_rgba8(255, 255, 255, 255));
    let mut dark = Paint::default();
    dark.anti_alias = true;
    dark.set_color(Color::from_rgba8(17, 17, 17, 255));

    // ── White halo over the whole spline ────────────────────────────────────
    if let Some(path) = {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].x, pts[0].y);
        for i in 0..n - 1 {
            let b = whip_segment_bezier(pts, i);
            pb.cubic_to(b.cp1x, b.cp1y, b.cp2x, b.cp2y, b.x2, b.y2);
        }
        pb.finish()
    } {
        pixmap.stroke_path(
            &path,
            &white,
            &stroke(rp.line_width_tip + rp.outline_width * 2.0),
            Transform::identity(),
            None,
        );
    }

    // ── Extra-fat white halo over the handle links only ──────────────────────
    let thick_links = rp.handle_thick_segments.min(n - 1);
    if thick_links > 0 && rp.handle_extra_width > 0.0 {
        if let Some(path) = {
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].x, pts[0].y);
            for i in 0..thick_links {
                let b = whip_segment_bezier(pts, i);
                pb.cubic_to(b.cp1x, b.cp1y, b.cp2x, b.cp2y, b.x2, b.y2);
            }
            pb.finish()
        } {
            pixmap.stroke_path(
                &path,
                &white,
                &stroke(rp.line_width_handle + rp.handle_extra_width + rp.outline_width * 2.0),
                Transform::identity(),
                None,
            );
        }
    }

    // ── Dark core, tapering handle→tip, one stroke per segment ───────────────
    for i in 0..n - 1 {
        let t = i as f32 / (n - 2).max(1) as f32;
        let extra = if i < rp.handle_thick_segments {
            rp.handle_extra_width
        } else {
            0.0
        };
        let width = lerp(rp.line_width_handle, rp.line_width_tip, t) + extra;
        if let Some(path) = {
            let mut pb = PathBuilder::new();
            pb.move_to(pts[i].x, pts[i].y);
            let b = whip_segment_bezier(pts, i);
            pb.cubic_to(b.cp1x, b.cp1y, b.cp2x, b.cp2y, b.x2, b.y2);
            pb.finish()
        } {
            pixmap.stroke_path(&path, &dark, &stroke(width), Transform::identity(), None);
        }
    }
}
