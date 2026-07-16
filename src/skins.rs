//! Whip skins — the pickable "materials" ported from OpenWhip's `SKINS`
//! (overlay.html). Each skin is either **flat** (a coloured halo over a solid
//! core — the original black-&-white look) or a shaded **tube** (a gradient
//! across each segment's width, a dark edge, and optional braid cross-hatch +
//! glowing seam dots). The chosen skin id is persisted next to the config so it
//! survives restarts; the tray "Skin" submenu drives the choice.

use crate::logging::log;
use tiny_skia::Color;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SkinType {
    /// Coloured halo/outline + solid core (the classic look).
    Flat,
    /// Each segment shaded across its width (shadow→highlight→mid), a dark
    /// `edge`, optional `braid` cross-hatch and glowing `seam` dots.
    Tube,
}

/// Soft aura around the rope (approximated from canvas `shadowBlur`).
#[derive(Clone, Copy)]
pub struct Glow {
    pub color: Color,
    pub blur: f32,
}

/// Diagonal cross-hatch ticks, alternating direction per segment.
#[derive(Clone, Copy)]
pub struct Braid {
    pub color: Color,
    pub alpha: f32,
}

/// Glowing dots spaced along the rope.
#[derive(Clone, Copy)]
pub struct Seam {
    pub color: Color,
    pub blur: f32,
    pub spacing: usize,
    pub size: f32,
}

/// One selectable whip material. `Copy` so the app can hold the current skin
/// by value and hand `&Skin` to the renderer each frame.
#[derive(Clone, Copy)]
pub struct Skin {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: SkinType,
    // Flat look.
    pub core: Color,
    pub outline: Color,
    // Tube look.
    pub edge: Option<Color>,
    pub shadow: Color,
    pub mid: Color,
    pub highlight: Color,
    // Shared decorations.
    pub glow: Option<Glow>,
    pub braid: Option<Braid>,
    pub seam: Option<Seam>,
}

/// `0xRRGGBB` → opaque [`Color`].
fn rgb(hex: u32) -> Color {
    Color::from_rgba8(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
        255,
    )
}

/// All skins, in tray-menu order. Mirrors the `SKINS` maps in OpenWhip's
/// `overlay.html` (visuals) and `main.js` (id + label). Built at runtime because
/// `Color::from_rgba8` isn't `const`.
pub fn all() -> Vec<Skin> {
    vec![
        Skin {
            id: "classic",
            label: "Classic — black & white",
            kind: SkinType::Flat,
            core: rgb(0x111111),
            outline: rgb(0xffffff),
            edge: None,
            shadow: rgb(0x111111),
            mid: rgb(0x111111),
            highlight: rgb(0xffffff),
            glow: None,
            braid: None,
            seam: None,
        },
        Skin {
            id: "notorious",
            label: "Notorious — braided leather + red glow",
            kind: SkinType::Tube,
            core: rgb(0x111111),
            outline: rgb(0xffffff),
            // Near-black leather with only a warm sheen in the highlight, so the
            // red seams/glow read as light through the weave rather than colour.
            edge: Some(rgb(0x040404)),
            shadow: rgb(0x0c0605),
            mid: rgb(0x22140f),
            highlight: rgb(0x6a3320),
            glow: Some(Glow {
                color: rgb(0xff2a2a),
                blur: 6.0,
            }),
            braid: Some(Braid {
                color: rgb(0x000000),
                alpha: 0.6,
            }),
            seam: Some(Seam {
                color: rgb(0xff342a),
                blur: 14.0,
                spacing: 3,
                size: 2.4,
            }),
        },
        Skin {
            id: "chrome",
            label: "Chrome — polished metal",
            kind: SkinType::Tube,
            core: rgb(0x111111),
            outline: rgb(0xffffff),
            edge: Some(rgb(0x0a0f14)),
            shadow: rgb(0x28323d),
            mid: rgb(0x93a2af),
            highlight: rgb(0xffffff),
            glow: None,
            braid: None,
            seam: None,
        },
        Skin {
            id: "gold",
            label: "Gold",
            kind: SkinType::Tube,
            core: rgb(0x111111),
            outline: rgb(0xffffff),
            edge: Some(rgb(0x2e1f00)),
            shadow: rgb(0x5a3d00),
            mid: rgb(0xcaa02a),
            highlight: rgb(0xfff3b8),
            glow: Some(Glow {
                color: rgb(0xffb300),
                blur: 8.0,
            }),
            braid: None,
            seam: Some(Seam {
                color: rgb(0xffd257),
                blur: 12.0,
                spacing: 5,
                size: 2.0,
            }),
        },
        Skin {
            id: "neon",
            label: "Neon — cyan",
            kind: SkinType::Tube,
            core: rgb(0x111111),
            outline: rgb(0xffffff),
            edge: Some(rgb(0x031a20)),
            shadow: rgb(0x053a46),
            mid: rgb(0x19c9e2),
            highlight: rgb(0xeaffff),
            glow: Some(Glow {
                color: rgb(0x00e0ff),
                blur: 20.0,
            }),
            braid: None,
            seam: None,
        },
    ]
}

/// Index of the skin with this id, or 0 (classic) if unknown.
pub fn index_of(id: &str) -> usize {
    all().iter().position(|s| s.id == id).unwrap_or(0)
}

/// Read the persisted skin id, or `"classic"` if none is saved / it's unknown.
pub fn load_selected_id() -> String {
    if let Some(p) = crate::config::skin_path()
        && let Ok(text) = std::fs::read_to_string(&p)
    {
        let id = text.trim();
        if all().iter().any(|s| s.id == id) {
            return id.to_string();
        }
    }
    "classic".to_string()
}

/// Persist the chosen skin id (a one-line file beside the config).
pub fn save_selected_id(id: &str) {
    let Some(p) = crate::config::skin_path() else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&p, id) {
        log!("agent-whip: could not save skin ({e})");
    }
}
