//! Icon generator: draws a bold tapered bullwhip with tiny-skia (the same
//! stroking style the app uses) and writes template + app-icon PNGs.
//!
//! Run: `cargo run --example gen_icon`

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

/// Control points of the whip, handle → tip, in a 1024×1024 box (y down).
/// A short thick handle at lower-left, a lash sweeping up and over to the right,
/// then snapping down to a fine tip.
const PTS: &[(f32, f32)] = &[
    (215.0, 815.0), // handle butt
    (275.0, 720.0),
    (330.0, 620.0), // grip → lash
    (370.0, 515.0),
    (445.0, 415.0),
    (560.0, 360.0),
    (690.0, 375.0), // apex, upper-right
    (785.0, 460.0),
    (795.0, 585.0),
    (720.0, 675.0), // snap down
    (660.0, 760.0),
    (690.0, 835.0), // fine tip, lower-right
];

const HANDLE_W: f32 = 92.0; // stroke width at the handle
const TIP_W: f32 = 12.0; // stroke width at the tip

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn catmull(i: i32) -> (f32, f32) {
    let n = PTS.len() as i32;
    if i < 0 {
        return (2.0 * PTS[0].0 - PTS[1].0, 2.0 * PTS[0].1 - PTS[1].1);
    }
    if i >= n {
        let a = PTS[(n - 2) as usize];
        let b = PTS[(n - 1) as usize];
        return (2.0 * b.0 - a.0, 2.0 * b.1 - a.1);
    }
    PTS[i as usize]
}

fn seg_bezier(i: usize) -> (f32, f32, f32, f32, f32, f32) {
    let (p0x, p0y) = catmull(i as i32 - 1);
    let p1 = PTS[i];
    let p2 = PTS[i + 1];
    let (p3x, p3y) = catmull(i as i32 + 2);
    (
        p1.0 + (p2.0 - p0x) / 6.0,
        p1.1 + (p2.1 - p0y) / 6.0,
        p2.0 - (p3x - p1.0) / 6.0,
        p2.1 - (p3y - p1.1) / 6.0,
        p2.0,
        p2.1,
    )
}

fn cubic(p0: (f32, f32), c1: (f32, f32), c2: (f32, f32), p1: (f32, f32), u: f32) -> (f32, f32) {
    let mu = 1.0 - u;
    let a = mu * mu * mu;
    let b = 3.0 * mu * mu * u;
    let c = 3.0 * mu * u * u;
    let d = u * u * u;
    (
        a * p0.0 + b * c1.0 + c * c2.0 + d * p1.0,
        a * p0.1 + b * c1.1 + c * c2.1 + d * p1.1,
    )
}

/// Draw the whip (black) into a 1024-space pixmap, applying `transform`.
fn draw_whip(pixmap: &mut Pixmap, transform: Transform, color: Color) {
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color(color);

    let n = PTS.len();
    // Sample the Catmull-Rom spline densely so the tapered stroke stays smooth.
    let steps = 16usize;
    let mut samples: Vec<(f32, f32, f32)> = Vec::new(); // x, y, global-param
    for i in 0..n - 1 {
        let (c1x, c1y, c2x, c2y, x2, y2) = seg_bezier(i);
        for s in 0..steps {
            let u = s as f32 / steps as f32;
            let (x, y) = cubic(PTS[i], (c1x, c1y), (c2x, c2y), (x2, y2), u);
            samples.push((x, y, (i as f32 + u) / (n - 1) as f32));
        }
    }
    samples.push((PTS[n - 1].0, PTS[n - 1].1, 1.0));

    // Tapered lash: fine overlapping round-capped segments → smooth taper.
    for w in samples.windows(2) {
        let width = lerp(HANDLE_W, TIP_W, w[0].2.powf(0.85));
        let mut pb = PathBuilder::new();
        pb.move_to(w[0].0, w[0].1);
        pb.line_to(w[1].0, w[1].1);
        if let Some(path) = pb.finish() {
            let stroke = Stroke {
                width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
        }
    }

    // Handle grip: a fat rounded cap so the butt reads as a handle.
    let mut grip = PathBuilder::new();
    grip.move_to(PTS[0].0, PTS[0].1);
    grip.line_to(PTS[1].0, PTS[1].1);
    if let Some(path) = grip.finish() {
        let stroke = Stroke {
            width: HANDLE_W + 14.0,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Crack: three short rays flicking off the tip.
    let tip = PTS[n - 1];
    let prev = PTS[n - 2];
    let dir = {
        let (dx, dy) = (tip.0 - prev.0, tip.1 - prev.1);
        let l = (dx * dx + dy * dy).sqrt().max(1.0);
        (dx / l, dy / l)
    };
    for (ang, len) in [(-0.55f32, 95.0f32), (0.0, 120.0), (0.55, 95.0)] {
        let (s, c) = ang.sin_cos();
        let rx = dir.0 * c - dir.1 * s;
        let ry = dir.0 * s + dir.1 * c;
        let start = (tip.0 + rx * 20.0, tip.1 + ry * 20.0);
        let end = (tip.0 + rx * len, tip.1 + ry * len);
        let mut ray = PathBuilder::new();
        ray.move_to(start.0, start.1);
        ray.line_to(end.0, end.1);
        if let Some(path) = ray.finish() {
            let stroke = Stroke {
                width: 16.0,
                line_cap: LineCap::Round,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
        }
    }
}

fn main() {
    // 1) Template (menu-bar) icon: black whip on transparent, 1024 & 512.
    for size in [1024u32, 512] {
        let mut pm = Pixmap::new(size, size).unwrap();
        let scale = size as f32 / 1024.0;
        draw_whip(
            &mut pm,
            Transform::from_scale(scale, scale),
            Color::from_rgba8(0, 0, 0, 255),
        );
        let path = format!("/tmp/whip_template_{size}.png");
        pm.save_png(&path).unwrap();
        println!("wrote {path}");
    }

    // 2) App-icon tile: whip on a rounded white square (macOS app-icon look).
    let size = 1024u32;
    let mut pm = Pixmap::new(size, size).unwrap();
    let mut bg = Paint::default();
    bg.anti_alias = true;
    bg.set_color(Color::from_rgba8(250, 250, 250, 255));
    let margin = 96.0;
    let rr = PathBuilder::from_rect(
        Rect::from_ltrb(margin, margin, size as f32 - margin, size as f32 - margin).unwrap(),
    );
    // rounded corners via a simple rounded-rect path
    let mut rrb = PathBuilder::new();
    let (l, t, r, b, rad) = (margin, margin, size as f32 - margin, size as f32 - margin, 180.0);
    rrb.move_to(l + rad, t);
    rrb.line_to(r - rad, t);
    rrb.quad_to(r, t, r, t + rad);
    rrb.line_to(r, b - rad);
    rrb.quad_to(r, b, r - rad, b);
    rrb.line_to(l + rad, b);
    rrb.quad_to(l, b, l, b - rad);
    rrb.line_to(l, t + rad);
    rrb.quad_to(l, t, l + rad, t);
    rrb.close();
    let _ = rr; // (kept the sharp-rect builder out; using rounded one)
    if let Some(path) = rrb.finish() {
        pm.fill_path(&path, &bg, FillRule::Winding, Transform::identity(), None);
    }
    draw_whip(&mut pm, Transform::identity(), Color::from_rgba8(20, 20, 20, 255));
    pm.save_png("/tmp/whip_appicon_1024.png").unwrap();
    println!("wrote /tmp/whip_appicon_1024.png");
}
