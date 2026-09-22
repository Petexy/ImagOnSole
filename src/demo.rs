//! A made-up folder of photographs, for pictures of this application.
//!
//! `--demo` exists for two reasons: to photograph the interface — every
//! picture in the README was taken with it — and to let somebody look at the
//! program on a machine with no photographs on it at all.
//!
//! **Nothing of the user's is read, and nothing of theirs is written.** The
//! folder is generated into this application's own cache and opened from
//! there, which means every page above it is the ordinary code path reading
//! ordinary files off a disk: the same directory walk, the same decoder, the
//! same thumbnail. A viewer photographed against a special case would be a
//! picture of the special case rather than of the viewer.
//!
//! The pictures are drawn rather than taken — there is no photograph to ship
//! and no photograph of anybody's to borrow — so they are scenes made out of
//! gradients and noise, at the resolutions a camera writes. They are
//! deterministic: the same build makes the same folder every time, so two
//! shots taken a week apart differ by what changed in the program and by
//! nothing else.

use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;

/// What the folder is called, and so what the head of the page reads.
///
/// It says what it is. A made-up folder wearing a real place's name is the one
/// screenshot somebody would go looking for the pictures from.
const FOLDER: &str = "Preview";

/// Bumped whenever a scene changes, which is what makes a stale cache
/// regenerate rather than being shown for ever.
const GENERATION: u32 = 6;

/// The made-up folder, generated if this machine has not got it already.
pub fn folder() -> Result<PathBuf, String> {
    let root = cache_root()?.join("preview");
    let folder = root.join(FOLDER);
    let stamp = root.join(format!(".generation-{GENERATION}"));
    if stamp.exists() && folder.is_dir() {
        return Ok(folder);
    }

    // A generation that is no longer wanted is removed outright rather than
    // written over: a renamed picture left behind from the build before would
    // appear in the listing and in the count, and be the one thing on the page
    // nothing in this file explains.
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&folder).map_err(|err| format!("{}: {err}", folder.display()))?;

    for picture in PICTURES {
        let path = match picture.within {
            Some(inner) => {
                let inner = folder.join(inner);
                std::fs::create_dir_all(&inner)
                    .map_err(|err| format!("{}: {err}", inner.display()))?;
                inner.join(picture.name)
            }
            None => folder.join(picture.name),
        };
        write(&path, picture)?;
    }

    std::fs::write(&stamp, b"").map_err(|err| format!("{}: {err}", stamp.display()))?;
    Ok(folder)
}

fn cache_root() -> Result<PathBuf, String> {
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME").filter(|it| !it.is_empty()) {
        return Ok(PathBuf::from(cache).join("imagonsole"));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| String::from("--demo needs somewhere to write, and HOME is not set"))?;
    Ok(PathBuf::from(home).join(".cache").join("imagonsole"))
}

/// One made-up photograph: what it is called, what shape it is, and what is in
/// it.
struct Picture {
    name: &'static str,
    /// The subfolder it lives in, so that the listing has a way further in and
    /// the count on the page has a folder to count.
    within: Option<&'static str>,
    width: u32,
    height: u32,
    scene: Scene,
    seed: u32,
}

/// The five kinds of scene, each with the light it is lit by.
///
/// Five rather than eleven because a scene is a palette and a time of day as
/// much as it is a shape: the same water under a different sun is a different
/// picture, and writing it twice would only mean two places to get it wrong.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scene {
    /// Sky over water, with a sun in it and a column of light under the sun.
    Water(Light),
    /// Ridges one behind another, each hazier than the one in front.
    Ridges(Light),
    /// A skyline at night, with windows in it.
    City,
    /// Sand, shaded by which way it leans.
    Dunes,
    /// Trunks in mist, which is the one scene that wants a portrait.
    Forest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Light {
    Dawn,
    Midday,
    Dusk,
    Cold,
}

const PICTURES: &[Picture] = &[
    Picture {
        name: "Harbour at dawn.jpg",
        within: None,
        width: 2400,
        height: 1600,
        scene: Scene::Water(Light::Dawn),
        seed: 11,
    },
    Picture {
        name: "The long ridge.jpg",
        within: None,
        width: 2400,
        height: 1600,
        scene: Scene::Ridges(Light::Dusk),
        seed: 23,
    },
    Picture {
        name: "First snow.jpg",
        within: None,
        width: 2400,
        height: 1600,
        scene: Scene::Ridges(Light::Cold),
        seed: 37,
    },
    Picture {
        name: "City, late.jpg",
        within: None,
        width: 2400,
        height: 1600,
        scene: Scene::City,
        seed: 41,
    },
    Picture {
        name: "Low tide.jpg",
        within: None,
        width: 2400,
        height: 1600,
        scene: Scene::Water(Light::Midday),
        seed: 53,
    },
    Picture {
        name: "Dunes at six.jpg",
        within: None,
        width: 2400,
        height: 1600,
        scene: Scene::Dunes,
        seed: 67,
    },
    // A camera's own numbering, and the pair that says which order a listing
    // is in: `IMG_0009` before `IMG_0010`, which plain byte order gets right
    // only because they were padded. The unpadded pair is in the subfolder.
    Picture {
        name: "IMG_0009.jpg",
        within: None,
        width: 1600,
        height: 2400,
        scene: Scene::Forest,
        seed: 71,
    },
    Picture {
        name: "IMG_0010.jpg",
        within: None,
        width: 1600,
        height: 2400,
        scene: Scene::Forest,
        seed: 83,
    },
    Picture {
        name: "Black sand.jpg",
        within: Some("Iceland"),
        width: 2400,
        height: 1600,
        scene: Scene::Water(Light::Cold),
        seed: 97,
    },
    Picture {
        name: "The pass.jpg",
        within: Some("Iceland"),
        width: 2400,
        height: 1600,
        scene: Scene::Ridges(Light::Midday),
        seed: 101,
    },
    Picture {
        name: "Midnight.jpg",
        within: Some("Iceland"),
        width: 2400,
        height: 1600,
        scene: Scene::Water(Light::Dusk),
        seed: 113,
    },
];

fn write(path: &Path, picture: &Picture) -> Result<(), String> {
    let pixels = render(picture);
    let file = std::fs::File::create(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut out = std::io::BufWriter::new(file);
    // Eighty-eight rather than the default, because these are looked at full
    // screen and at actual size: the one place a demo may not cut a corner is
    // the thing the program exists to show.
    JpegEncoder::new_with_quality(&mut out, 88)
        .encode(
            &pixels,
            picture.width,
            picture.height,
            ExtendedColorType::Rgb8,
        )
        .map_err(|err| format!("{}: {err}", path.display()))
}

fn render(picture: &Picture) -> Vec<u8> {
    let (width, height) = (picture.width as f32, picture.height as f32);
    let mut pixels = Vec::with_capacity((picture.width * picture.height * 3) as usize);
    for y in 0..picture.height {
        let v = (y as f32 + 0.5) / height;
        for x in 0..picture.width {
            let u = (x as f32 + 0.5) / width;
            let mut colour = match picture.scene {
                Scene::Water(light) => water(u, v, light, picture.seed),
                Scene::Ridges(light) => ridges(u, v, light, picture.seed),
                Scene::City => city(u, v, picture.seed),
                Scene::Dunes => dunes(u, v, picture.seed),
                Scene::Forest => forest(u, v, picture.seed),
            };
            // A lens is darker at its edges than at its middle, and a picture
            // without that reads as a rendering of a scene rather than as a
            // photograph of one.
            let corner = ((u - 0.5) * 1.35).powi(2) + ((v - 0.5) * 1.35).powi(2);
            let vignette = 1.0 - 0.34 * corner;
            // And a sensor is never quite silent. A little grain is what stops
            // a wide gradient banding once it is JPEG'd.
            let grain = (value(x as i32, y as i32, picture.seed ^ 0x9e37) - 0.5) * 0.012;
            for channel in &mut colour {
                *channel = (*channel * vignette + grain).clamp(0.0, 1.0);
            }
            pixels.extend_from_slice(&[byte(colour[0]), byte(colour[1]), byte(colour[2])]);
        }
    }
    pixels
}

fn byte(value: f32) -> u8 {
    (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

// ---------------------------------------------------------------- the scenes

/// The three colours a sky is made of, and the sun that is in it.
struct Sky {
    top: [f32; 3],
    middle: [f32; 3],
    horizon: [f32; 3],
    sun: [f32; 3],
    /// Where the sun stands, across and down. Below the horizon is a sun that
    /// has set, which is what leaves the glow without the disc.
    sun_at: [f32; 2],
    /// How large the disc is, as a fraction of the width.
    sun_size: f32,
    deep: [f32; 3],
}

fn sky_of(light: Light) -> Sky {
    match light {
        Light::Dawn => Sky {
            top: [0.07, 0.11, 0.26],
            middle: [0.36, 0.26, 0.42],
            horizon: [0.96, 0.66, 0.38],
            sun: [1.0, 0.93, 0.74],
            sun_at: [0.66, 0.50],
            sun_size: 0.030,
            deep: [0.05, 0.08, 0.15],
        },
        Light::Midday => Sky {
            top: [0.16, 0.38, 0.68],
            middle: [0.40, 0.62, 0.82],
            horizon: [0.76, 0.85, 0.90],
            sun: [1.0, 0.99, 0.94],
            sun_at: [0.28, 0.16],
            sun_size: 0.018,
            deep: [0.10, 0.27, 0.36],
        },
        Light::Dusk => Sky {
            top: [0.04, 0.05, 0.14],
            middle: [0.17, 0.12, 0.29],
            horizon: [0.55, 0.26, 0.31],
            sun: [1.0, 0.78, 0.55],
            sun_at: [0.38, 0.575],
            sun_size: 0.026,
            deep: [0.03, 0.04, 0.09],
        },
        Light::Cold => Sky {
            top: [0.20, 0.30, 0.44],
            middle: [0.44, 0.55, 0.66],
            horizon: [0.72, 0.78, 0.83],
            sun: [0.96, 0.96, 0.98],
            sun_at: [0.74, 0.30],
            sun_size: 0.016,
            deep: [0.09, 0.13, 0.18],
        },
    }
}

/// The sky alone, above a horizon at `horizon`.
fn sky_at(u: f32, v: f32, horizon: f32, sky: &Sky, seed: u32, aspect: f32) -> [f32; 3] {
    let t = (v / horizon).clamp(0.0, 1.0);
    let mut colour = if t < 0.55 {
        mix(sky.top, sky.middle, smoothstep(t / 0.55))
    } else {
        mix(sky.middle, sky.horizon, smoothstep((t - 0.55) / 0.45))
    };

    // Cloud, as bands that are stretched flat near the horizon — which is what
    // perspective does to anything lying in a layer.
    let stretch = 1.0 + 6.0 * t * t;
    let cloud = fbm(u * 2.6, v * 5.0 * stretch, seed ^ 0x51ed, 4);
    let amount = 0.30 * smoothstep(((cloud - 0.46) / 0.30).clamp(0.0, 1.0)) * (0.25 + 0.75 * t);
    colour = mix(colour, lighten(sky.horizon, 0.25), amount);

    // The sun: a disc, and a glow that reaches much further than the disc
    // does. The glow is what says which way the light is coming from in every
    // other part of the picture, so it is added rather than mixed.
    let dx = (u - sky.sun_at[0]) * aspect;
    let dy = v - sky.sun_at[1];
    let distance = (dx * dx + dy * dy).sqrt();
    let glow = (-distance / 0.16).exp() * 0.9;
    colour = add(colour, scale(sky.sun, glow * 0.55));
    if distance < sky.sun_size && sky.sun_at[1] < horizon {
        let edge = smoothstep(((sky.sun_size - distance) / (sky.sun_size * 0.35)).clamp(0.0, 1.0));
        colour = mix(colour, sky.sun, edge);
    }
    colour
}

fn water(u: f32, v: f32, light: Light, seed: u32) -> [f32; 3] {
    const HORIZON: f32 = 0.56;
    let sky = sky_of(light);
    if v < HORIZON {
        return sky_at(u, v, HORIZON, &sky, seed, 1.5);
    }

    // Below the horizon the sky is what is being reflected, so the water
    // starts as the sky's own colours and darkens with distance from the eye.
    let down = ((v - HORIZON) / (1.0 - HORIZON)).clamp(0.0, 1.0);
    let mut colour = mix(darken(sky.horizon, 0.35), sky.deep, smoothstep(down));

    // Ripples: bands that grow further apart towards the bottom of the frame,
    // because the near water is closer to the eye than the far water is.
    let spacing = 26.0 + 150.0 * down * down;
    let wobble = fbm(u * 3.0, v * 9.0, seed ^ 0x2f1b, 3) * 5.5;
    let band = ((v * spacing + wobble).sin() * 0.5 + 0.5).powf(2.2);
    colour = add(
        colour,
        scale(lighten(sky.horizon, 0.2), band * 0.16 * (0.25 + down)),
    );

    // And the column under the sun, which widens as it comes towards the eye
    // and is the brightest thing in the picture.
    let width = 0.02 + 0.34 * down;
    let across = ((u - sky.sun_at[0]) / width).abs();
    if across < 1.0 {
        let strength = (1.0 - across).powi(2) * (1.0 - 0.45 * down);
        colour = add(colour, scale(sky.sun, strength * band * 0.85));
    }
    colour
}

fn ridges(u: f32, v: f32, light: Light, seed: u32) -> [f32; 3] {
    const HORIZON: f32 = 0.72;
    let sky = sky_of(light);
    let mut colour = sky_at(u, v, HORIZON, &sky, seed, 1.5);

    // Painter's order, far to near: each layer stands lower on the frame, is
    // rougher than the one behind it, and has less of the sky's haze mixed
    // into it. That mixture is the whole of the depth here.
    let rock = match light {
        Light::Cold => [0.21, 0.24, 0.31],
        Light::Midday => [0.24, 0.31, 0.26],
        _ => [0.16, 0.13, 0.20],
    };
    for layer in 0..4 {
        let far = 1.0 - layer as f32 / 3.0;
        let base = 0.40 + 0.16 * layer as f32;
        let amplitude = 0.05 + 0.055 * layer as f32;
        let frequency = 1.6 + 2.2 * layer as f32;
        let height =
            base - amplitude * (ridge(u * frequency, seed.wrapping_add(layer as u32 * 17)) - 0.45);
        if v < height {
            continue;
        }
        let mut here = mix(rock, sky.horizon, far * 0.62);
        here = darken(here, (1.0 - far) * 0.35);

        // Snow lies along the tops, and lies deeper the higher the top is —
        // so it is the shape somebody can see that puts it there, rather than
        // a second noise they have no way to tie to the ridge.
        if light == Light::Cold {
            let below = ((v - height) / 0.055).clamp(0.0, 1.0);
            let high = smoothstep(((0.58 - height) / 0.16).clamp(0.0, 1.0));
            let cap = smoothstep((1.0 - below) * high);
            let snow = mix([0.93, 0.95, 0.98], sky.horizon, far * 0.55);
            here = mix(here, snow, cap * 0.95);
        }
        colour = here;
    }
    colour
}

fn city(u: f32, v: f32, seed: u32) -> [f32; 3] {
    // A night sky, and the orange the ground throws back up into it — which is
    // why a city never has a black sky over it.
    let mut colour = mix([0.03, 0.04, 0.10], [0.16, 0.11, 0.16], smoothstep(v / 0.78));
    colour = add(
        colour,
        scale(
            [0.55, 0.32, 0.16],
            (v / 0.78).clamp(0.0, 1.0).powi(3) * 0.55,
        ),
    );

    // Stars, in the top of the frame only, where the glow has not washed them
    // out. One pixel each, so they survive being scaled down to a card.
    if v < 0.42 {
        let star = value((u * 900.0) as i32, (v * 600.0) as i32, seed ^ 0x77af);
        if star > 0.9992 {
            colour = add(colour, [0.7, 0.7, 0.8]);
        }
    }

    // Two ranks of buildings. The far rank is hazier and lower; the near rank
    // is nearly black, which is what an eye adjusted to a lit window sees.
    for rank in 0..2 {
        let near = rank == 1;
        let skyline = if near { 0.56 } else { 0.44 };
        let width = if near { 0.085 } else { 0.055 };
        let block = (u / width).floor();
        let key = seed
            .wrapping_add(rank as u32 * 977)
            .wrapping_add(block as u32);
        let top = skyline - 0.26 * value(block as i32, rank, key) - if near { 0.0 } else { 0.02 };
        if v < top {
            continue;
        }
        let body = if near {
            [0.035, 0.035, 0.055]
        } else {
            [0.10, 0.10, 0.15]
        };
        colour = body;

        // Windows. A grid inside the block, a margin so the rows do not run
        // into the edges, and a third of them lit — a building with every
        // window lit is an office nobody has ever worked in.
        let across = ((u - block * width) / width - 0.5).abs();
        if across > 0.40 {
            continue;
        }
        let column = ((u - block * width) / (width / 6.0)).floor();
        let row = ((v - top) / 0.020).floor();
        let inside_column = ((u - block * width) / (width / 6.0)).fract();
        let inside_row = ((v - top) / 0.020).fract();
        if !(0.22..0.78).contains(&inside_column) || !(0.25..0.75).contains(&inside_row) {
            continue;
        }
        let lit = value(column as i32 + block as i32 * 97, row as i32, key ^ 0x1234);
        if lit > 0.66 {
            let warmth = 0.55 + 0.45 * value(row as i32, column as i32, key ^ 0x5678);
            let window = [1.0, 0.82, 0.52];
            colour = mix(colour, window, if near { 0.85 } else { 0.55 } * warmth);
        }
    }
    colour
}

fn dunes(u: f32, v: f32, seed: u32) -> [f32; 3] {
    const HORIZON: f32 = 0.30;
    let sky = sky_of(Light::Dusk);
    if v < HORIZON {
        return sky_at(u, v, HORIZON, &sky, seed, 1.5);
    }

    // The haze at the foot of the sky, so that the gap between two crests is
    // distance rather than a hole in the picture.
    let sun = sky.sun_at[0];
    let mut colour = darken(sky.horizon, 0.30);

    // Sand, far to near. What makes a dune read as a dune and not as a stripe
    // is that the face turned away from the sun is in shade — and that shade
    // is read off the *slope* of the crest at this point. Which is why a crest
    // is two slow waves and one slow octave of noise and nothing rougher: the
    // slope of a rough curve is a stripe, and the picture would be combed.
    for layer in 0..5 {
        let near = layer as f32 / 4.0;
        let base = HORIZON + 0.02 + 0.175 * layer as f32;
        let frequency = 0.9 + 1.0 * layer as f32;
        let phase = seed.wrapping_add(layer as u32 * 31);
        let turn = value(layer, 0, seed) * std::f32::consts::TAU;
        let amplitude = 0.018 + 0.05 * near;
        let crest = |x: f32| {
            let wave = ((x * frequency * std::f32::consts::TAU + turn).sin() * 0.6
                + (x * frequency * 2.7 + turn * 1.7).sin() * 0.4)
                * 0.5;
            base - amplitude * (wave + (noise(x * frequency * 1.1, 0.5, phase) - 0.5))
        };
        let top = crest(u);
        if v < top {
            continue;
        }
        // Read either side, and far enough either side that what comes back is
        // the lie of the dune rather than the grain on it.
        let step = 0.02;
        let slope = (crest(u + step) - crest(u - step)) / (2.0 * step);
        // One direction for the whole frame. Taking the sun's side *per point*
        // flips the sign as the picture passes under it, and a sign that flips
        // is a seam straight down the middle of the sand.
        let towards = if sun < 0.5 { -1.0 } else { 1.0 };
        let facing = (slope * towards * 2.6).clamp(-1.0, 1.0);

        let mut here = mix([0.60, 0.44, 0.37], [0.84, 0.58, 0.36], near);
        here = mix(here, darken(sky.horizon, 0.12), (1.0 - near).powi(2) * 0.66);
        here = scale(here, (1.0 + 0.30 * facing).clamp(0.55, 1.30));
        // Ripples run along the crest rather than down it, which is the one
        // thing that says which way the wind was going.
        let ripple = (fbm(u * 22.0, (v - top) * 80.0, phase ^ 0x3c3c, 2) - 0.5) * 0.06 * near;
        here = add(here, [ripple, ripple * 0.86, ripple * 0.7]);
        // And the near face falls into shadow as it comes down.
        here = darken(here, ((v - top) * 0.7).clamp(0.0, 0.22));
        colour = here;
    }
    colour
}

fn forest(u: f32, v: f32, seed: u32) -> [f32; 3] {
    // Mist, which is what a forest in the morning mostly is, and which is also
    // what this picture is lit by: everything else in it is dark against this.
    let mut colour = mix([0.84, 0.87, 0.82], [0.13, 0.19, 0.14], smoothstep(v * 1.05));

    // The canopy, across the top of the frame only — enough to say the light
    // is coming down through something rather than out of an open sky.
    let leaves = fbm(u * 5.0, v * 9.0, seed ^ 0x77bb, 4);
    let canopy = smoothstep(((0.20 - v) / 0.24).clamp(0.0, 1.0));
    colour = mix(
        colour,
        mix([0.08, 0.17, 0.09], [0.38, 0.50, 0.26], leaves),
        canopy * 0.90,
    );

    // Shafts of light, on the diagonal it comes in on. Added before the
    // trunks, so that a trunk stands in front of one rather than being lit by
    // it from the wrong side.
    let shaft = ((u * 4.2 + v * 2.0) * 2.1).sin() * 0.5 + 0.5;
    colour = add(
        colour,
        scale([0.86, 0.85, 0.60], shaft.powi(8) * 0.30 * (1.0 - v * 0.7)),
    );

    // Three ranks, and the whole of the depth is the difference between them.
    // The far rank is many, hair-thin and nearly the colour of the mist — it
    // is a distance rather than a thing to look at. The near rank is four,
    // dark, and *narrow enough to leave the mist showing between them*. One
    // rank of evenly graded trunks is a set of blinds, and so is a near rank
    // as wide as the gap between its trunks; this looked like both in turn.
    for (count, from, to, spacing) in [
        (12u32, 0.08f32, 0.30f32, 0.0f32),
        (5, 0.42, 0.58, 0.0),
        (3, 0.76, 1.0, 0.32),
    ] {
        for trunk in 0..count {
            let key = seed.wrapping_add((count * 7717).wrapping_add(trunk * 131));
            let jitter = value(trunk as i32, 1, key);
            let along = if count > 1 {
                trunk as f32 / (count - 1) as f32
            } else {
                0.5
            };
            let depth = from + (to - from) * if spacing > 0.0 { along } else { jitter };
            // Spread rather than scattered: a random position alone clumps,
            // and a clump of trunks is the wall this is trying not to be.
            let at_foot = if spacing > 0.0 {
                0.12 + trunk as f32 * spacing + (jitter - 0.5) * 0.08
            } else {
                (jitter * 0.30 + trunk as f32 * 0.37).fract()
            };
            let lean = (value(trunk as i32, 2, key ^ 0xbeef) - 0.5) * 0.05;
            let at = at_foot
                + lean * (v - 0.5) * 2.0
                + 0.006 * (fbm(v * 2.5, trunk as f32, key, 2) - 0.5);
            // Wider at the foot than at the head, because a tree is.
            let width = (0.0030 + 0.055 * depth * depth) * (0.74 + 0.42 * v);
            let across = (u - at).abs() / width;
            if across > 1.0 {
                continue;
            }
            // Round, so that it is a cylinder lit down one edge rather than a
            // stripe of two colours. The lit edge is *narrow* — light wrapping
            // half way round a trunk is what made these read as fabric.
            let round = (1.0 - across * across).max(0.0).sqrt();
            let side = ((u - at) / width).clamp(-1.0, 1.0);
            let dark = mix([0.30, 0.27, 0.22], [0.045, 0.050, 0.042], depth);
            let bark = mix(
                dark,
                lighten(dark, 0.26),
                ((0.30 - side) * 1.3).clamp(0.0, 1.0),
            );
            // And the further one stands, the more mist there is in front of
            // it — which for the far rank is nearly all of it.
            let hazy = mix(bark, [0.78, 0.82, 0.78], (1.0 - depth).powi(2) * 0.97);
            colour = mix(colour, hazy, smoothstep((round * 2.6).clamp(0.0, 1.0)));
        }
    }

    // The floor the trunks stand on, and the fog lying over it.
    let floor = smoothstep(((v - 0.74) / 0.26).clamp(0.0, 1.0));
    let ground = mix(
        [0.14, 0.12, 0.09],
        [0.33, 0.35, 0.28],
        fbm(u * 6.0, v * 10.0, seed ^ 0x9a1, 3),
    );
    colour = mix(colour, ground, floor * 0.80);
    colour
}

// ------------------------------------------------------------ the arithmetic

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    [a[0] * by, a[1] * by, a[2] * by]
}

fn lighten(a: [f32; 3], by: f32) -> [f32; 3] {
    mix(a, [1.0, 1.0, 1.0], by)
}

fn darken(a: [f32; 3], by: f32) -> [f32; 3] {
    mix(a, [0.0, 0.0, 0.0], by)
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A number between nought and one for a point on a lattice, out of nothing
/// but the point itself — so the same build draws the same picture on every
/// machine, with no state carried from one pixel to the next.
fn value(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x8da6_b343)
        .wrapping_add((y as u32).wrapping_mul(0xd8163841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}

/// Value noise: the lattice, read between its points.
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (smoothstep(x - x0), smoothstep(y - y0));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let top = value(ix, iy, seed) + (value(ix + 1, iy, seed) - value(ix, iy, seed)) * fx;
    let bottom =
        value(ix, iy + 1, seed) + (value(ix + 1, iy + 1, seed) - value(ix, iy + 1, seed)) * fx;
    top + (bottom - top) * fy
}

/// Several octaves of it, which is what turns a lattice into cloud, haze or
/// grain depending on the scale it is asked for.
fn fbm(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let mut total = 0.0;
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    let mut weight = 0.0;
    for octave in 0..octaves {
        total += noise(x * frequency, y * frequency, seed.wrapping_add(octave * 7)) * amplitude;
        weight += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    total / weight.max(f32::EPSILON)
}

/// A skyline: one dimension of noise, with the smooth octaves folded so that
/// the peaks come to a point. A ridge drawn from plain noise is a row of
/// hills, and a mountain is not a hill.
fn ridge(x: f32, seed: u32) -> f32 {
    let mut total = 0.0;
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    let mut weight = 0.0;
    for octave in 0..5 {
        let here = noise(x * frequency, 0.5, seed.wrapping_add(octave * 13));
        total += (1.0 - (here * 2.0 - 1.0).abs()) * amplitude;
        weight += amplitude;
        amplitude *= 0.55;
        frequency *= 2.0;
    }
    total / weight.max(f32::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folder has something in it, has somewhere further in, and has the
    /// pair that says which order a listing is read in.
    #[test]
    fn the_made_up_folder_is_a_folder_somebody_could_have() {
        assert!(PICTURES.len() >= 8);
        assert!(PICTURES.iter().any(|one| one.within.is_some()));
        assert!(PICTURES.iter().any(|one| one.name == "IMG_0009.jpg"));
        assert!(PICTURES
            .iter()
            .all(|one| crate::library::is_photograph(Path::new(one.name))));
    }

    /// Every name is its own, in its own folder — two pictures of the same
    /// name would be one picture and a listing that counts wrong.
    #[test]
    fn no_two_pictures_share_a_name() {
        let mut seen: Vec<(Option<&str>, &str)> =
            PICTURES.iter().map(|one| (one.within, one.name)).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), before);
    }

    /// Nothing a scene draws leaves the range a colour has. Clamping at the
    /// end would hide it; a scene that goes out of range comes out flat white
    /// in the highlights, which is exactly what a photograph must not do.
    #[test]
    fn every_scene_stays_inside_a_colour() {
        for picture in PICTURES {
            for step in 0..64 {
                let t = step as f32 / 63.0;
                for (u, v) in [(t, 0.12), (t, 0.5), (t, 0.88), (0.5, t)] {
                    let colour = match picture.scene {
                        Scene::Water(light) => water(u, v, light, picture.seed),
                        Scene::Ridges(light) => ridges(u, v, light, picture.seed),
                        Scene::City => city(u, v, picture.seed),
                        Scene::Dunes => dunes(u, v, picture.seed),
                        Scene::Forest => forest(u, v, picture.seed),
                    };
                    for channel in colour {
                        assert!(
                            channel.is_finite() && (-0.01..1.45).contains(&channel),
                            "{} at ({u}, {v}) answered {channel}",
                            picture.name
                        );
                    }
                }
            }
        }
    }

    /// The same build draws the same picture. Two shots taken a week apart
    /// have to differ by what changed in the program and by nothing else.
    #[test]
    fn a_scene_is_the_same_every_time_it_is_asked_for() {
        let once = water(0.31, 0.44, Light::Dawn, 11);
        let again = water(0.31, 0.44, Light::Dawn, 11);
        assert_eq!(once, again);
        assert_ne!(once, water(0.31, 0.44, Light::Dusk, 11));
    }
}
