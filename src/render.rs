//! Whip rendering — ports `overlay.html`'s `draw()` (and its skins) to
//! `tiny-skia`.
//!
//! The rope is a Catmull-Rom spline through the chain points, rendered as cubic
//! béziers. A **flat** skin draws a coloured halo then a solid tapering core
//! (the classic look). A **tube** skin shades each segment across its width with
//! a linear gradient, adds a dark edge, and optionally a braid cross-hatch and
//! glowing seam dots. Canvas `shadowBlur` glows have no direct tiny-skia
//! equivalent, so they're approximated with a few translucent over-wide passes.

use crate::skins::{Braid, Glow, Seam, Skin, SkinType};
use crate::whip::{Point, Sim};
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder,
    Pixmap, Point as SkPoint, SpreadMode, Stroke, Transform,
};

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

/// `color` with its alpha replaced (0..1).
fn with_alpha(mut color: Color, alpha: f32) -> Color {
    color.set_alpha(alpha.clamp(0.0, 1.0));
    color
}

fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    }
}

/// An anti-aliased solid-colour paint.
fn solid(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.anti_alias = true;
    p.set_color(color);
    p
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

/// Segment width at index `i` (the handle→tip taper, thickened over the handle
/// links). Matches `segWidth` in `overlay.html`.
fn seg_width(rp: &RenderParams, i: usize, n: usize) -> f32 {
    let t = i as f32 / (n - 2).max(1) as f32;
    let extra = if i < rp.handle_thick_segments {
        rp.handle_extra_width
    } else {
        0.0
    };
    lerp(rp.line_width_handle, rp.line_width_tip, t) + extra
}

/// Spline path from the handle through the first `count` links (`count = n-1`
/// gives the whole rope).
fn spline_path(pts: &[Point], count: usize) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].x, pts[0].y);
    for i in 0..count {
        let b = whip_segment_bezier(pts, i);
        pb.cubic_to(b.cp1x, b.cp1y, b.cp2x, b.cp2y, b.x2, b.y2);
    }
    pb.finish()
}

/// Path for a single segment `i`.
fn segment_path(pts: &[Point], i: usize) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[i].x, pts[i].y);
    let b = whip_segment_bezier(pts, i);
    pb.cubic_to(b.cp1x, b.cp1y, b.cp2x, b.cp2y, b.x2, b.y2);
    pb.finish()
}

/// Clear the pixmap and, if a whip is present, draw it in the given skin.
pub fn draw(pixmap: &mut Pixmap, sim: &Sim, rp: &RenderParams, skin: &Skin) {
    pixmap.fill(Color::TRANSPARENT);

    if !sim.active || sim.pts.len() < 2 {
        return;
    }
    match skin.kind {
        SkinType::Flat => draw_flat(pixmap, &sim.pts, rp, skin),
        SkinType::Tube => draw_tube(pixmap, &sim.pts, rp, skin),
    }
}

/// Classic look: a coloured halo (extra-fat over the handle) + a solid tapering
/// core, with an optional glow underneath.
fn draw_flat(pixmap: &mut Pixmap, pts: &[Point], rp: &RenderParams, skin: &Skin) {
    let n = pts.len();

    if let Some(glow) = skin.glow {
        draw_glow(pixmap, pts, glow, rp.line_width_tip);
    }

    let outline = solid(skin.outline);

    // Halo over the whole spline.
    if let Some(path) = spline_path(pts, n - 1) {
        pixmap.stroke_path(
            &path,
            &outline,
            &stroke(rp.line_width_tip + rp.outline_width * 2.0),
            Transform::identity(),
            None,
        );
    }
    // Extra-fat halo over the handle links only.
    let thick = rp.handle_thick_segments.min(n - 1);
    if thick > 0
        && rp.handle_extra_width > 0.0
        && let Some(path) = spline_path(pts, thick)
    {
        pixmap.stroke_path(
            &path,
            &outline,
            &stroke(rp.line_width_handle + rp.handle_extra_width + rp.outline_width * 2.0),
            Transform::identity(),
            None,
        );
    }

    // Dark core, tapering handle→tip, one stroke per segment.
    let core = solid(skin.core);
    for i in 0..n - 1 {
        if let Some(path) = segment_path(pts, i) {
            pixmap.stroke_path(
                &path,
                &core,
                &stroke(seg_width(rp, i, n)),
                Transform::identity(),
                None,
            );
        }
    }
}

/// Material look: each segment is a shaded tube (shadow→highlight→mid across its
/// width) with a dark edge, optional braid cross-hatch, and glowing seam dots.
fn draw_tube(pixmap: &mut Pixmap, pts: &[Point], rp: &RenderParams, skin: &Skin) {
    let n = pts.len();

    if let Some(glow) = skin.glow {
        draw_glow(pixmap, pts, glow, rp.line_width_tip);
    }

    for i in 0..n - 1 {
        let a = pts[i];
        let b = pts[i + 1];
        let w = seg_width(rp, i, n);

        // Dark edge for definition.
        if let Some(edge) = skin.edge
            && let Some(path) = segment_path(pts, i)
        {
            pixmap.stroke_path(
                &path,
                &solid(edge),
                &stroke(w + 2.5),
                Transform::identity(),
                None,
            );
        }

        // Shaded tube: linear gradient across the segment's width.
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let (pxu, pyu) = (-dy / len, dx / len);
        let (mx, my) = ((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        let hw = w * 0.5;
        let stops = vec![
            GradientStop::new(0.0, skin.shadow),
            GradientStop::new(0.34, skin.highlight),
            GradientStop::new(0.58, skin.mid),
            GradientStop::new(1.0, skin.shadow),
        ];
        // `solid(skin.mid)` is the fallback if the gradient degenerates.
        let mut p = solid(skin.mid);
        if let Some(shader) = LinearGradient::new(
            SkPoint::from_xy(mx - pxu * hw, my - pyu * hw),
            SkPoint::from_xy(mx + pxu * hw, my + pyu * hw),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ) {
            p.shader = shader;
        }
        if let Some(path) = segment_path(pts, i) {
            pixmap.stroke_path(&path, &p, &stroke(w), Transform::identity(), None);
        }

        if let Some(braid) = skin.braid {
            draw_braid_tick(pixmap, (mx, my), (dx, dy), w, braid);
        }
    }

    // Glowing seam dots spaced along the rope.
    if let Some(seam) = skin.seam {
        let mut i = 1;
        while i < n - 1 {
            draw_seam_dot(pixmap, pts[i].x, pts[i].y, seam);
            i += seam.spacing.max(1);
        }
    }
}

/// A crossed braid stitch (an `X`) at a segment midpoint `mid`, giving the woven
/// leather look — chained down the rope the X's read as a diamond weave. `d` is
/// the segment's direction vector.
fn draw_braid_tick(pixmap: &mut Pixmap, mid: (f32, f32), d: (f32, f32), w: f32, braid: Braid) {
    let base = d.1.atan2(d.0);
    let l = w * 0.8;
    let paint = solid(with_alpha(braid.color, braid.alpha));
    let width = (w * 0.16).max(1.5);
    for sign in [1.0f32, -1.0] {
        let ta = base + sign * 0.7;
        let mut pb = PathBuilder::new();
        pb.move_to(mid.0 - ta.cos() * l, mid.1 - ta.sin() * l);
        pb.line_to(mid.0 + ta.cos() * l, mid.1 + ta.sin() * l);
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &paint, &stroke(width), Transform::identity(), None);
        }
    }
}

/// Fake a canvas `shadowBlur` aura: a few translucent, over-wide passes of the
/// glow colour along the whole spline. Cheap bloom without a real blur filter.
fn draw_glow(pixmap: &mut Pixmap, pts: &[Point], glow: Glow, base_width: f32) {
    let Some(path) = spline_path(pts, pts.len() - 1) else {
        return;
    };
    for (mult, alpha) in [(1.0f32, 0.06f32), (0.55, 0.11), (0.25, 0.17)] {
        pixmap.stroke_path(
            &path,
            &solid(with_alpha(glow.color, alpha)),
            &stroke(base_width + glow.blur * mult),
            Transform::identity(),
            None,
        );
    }
}

/// A glowing seam dot: translucent halo rings + a solid centre.
fn draw_seam_dot(pixmap: &mut Pixmap, x: f32, y: f32, seam: Seam) {
    for (mult, alpha) in [(0.55f32, 0.07f32), (0.3, 0.13), (0.14, 0.22)] {
        if let Some(path) = PathBuilder::from_circle(x, y, seam.size + seam.blur * mult) {
            let p = solid(with_alpha(seam.color, alpha));
            pixmap.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        }
    }
    if let Some(path) = PathBuilder::from_circle(x, y, seam.size) {
        let p = solid(seam.color);
        pixmap.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
}
