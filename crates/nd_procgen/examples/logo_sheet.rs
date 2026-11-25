//! [r17-logos] A contact sheet of generated company logos and two ad
//! frames, as a PNG: `cargo run -p nd_procgen --example logo_sheet -- out.png`.
//! Six rows of five logos (mega-corp marks by colour, shape and initial,
//! then chains and local businesses by name), each with its name under it
//! in the stroke font, then a row of billboard frames.

use glam::Vec2;
use nd_procgen::ads;
use nd_procgen::logo::{self, Ink, Mark, Paint, Tess};
use nd_sim::corp::LogoShape;

const CELL: usize = 200;
const COLS: usize = 5;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "logo_sheet.png".into());
    let corps: Vec<(Mark, String)> = (0..15u64)
        .map(|k| {
            let h = nd_sim::hash::mix(0x5EE7, k);
            let name = nd_sim::corp::corp_name(h);
            let colour = (k * 5 % 12) as u8;
            let shape = LogoShape::CORP[(k as usize * 3) % 7];
            (Mark::corp(colour, shape, name.chars().next().unwrap()), name)
        })
        .collect();
    let locals = [
        "Rocket Burger #3", "Lucky Noodle Bar", "Okafor Builders", "Pie Hard", "Starlite Diner", "Pixel Arcade", "Mama Chen's Ramen", "Iron Gym", "Velvet Bar", "Boba Tea Bar", "Meridian Fleet Lot", "Big Sky Pizza", "Insert Coin",
        "Saltwind Fisheries", "The Greasy Spoon",
    ];
    let mut marks = corps;
    marks.extend(locals.iter().map(|n| (Mark::local(n), logo::trading_name(n).to_string())));
    let rows = marks.len().div_ceil(COLS);
    let ad_h = 260;
    let w = CELL * COLS;
    let h = CELL * rows + ad_h * 2;
    let mut buf = vec![[0.015f32, 0.016, 0.022]; w * h];
    for (i, (m, name)) in marks.iter().enumerate() {
        let (cx, cy) = ((i % COLS) * CELL, (i / COLS) * CELL);
        let t = logo::tessellate(m);
        let paint = Paint::lit(m, 1.0);
        logo::rasterize(&mut buf, w, cx + 30, cy + 8, CELL - 60, &t, &paint);
        // The name under it.
        let ty = logo::type_of(m);
        let nh = (1.7 / logo::text_w(&name.to_uppercase(), 1.0, ty)).min(0.13);
        let nw = logo::text_w(&name.to_uppercase(), nh, ty);
        let (p, _) = logo::text_prims(&name.to_uppercase(), -nw * 0.5, -0.98, nh, logo::Type { weight: 0.14, ..ty }, Ink::Light);
        let mut tt = Tess::default();
        tt.add(&p);
        logo::rasterize(&mut buf, w, cx, cy, CELL, &tt, &Paint { light: [0.75, 0.75, 0.72], ..paint });
    }
    // Ad frames: two screens' frames side by side.
    let shows = [ads::fallback_ads(11), vec![ads::Ad { mark: Mark::local("Rocket Burger"), name: "Rocket Burger".into(), slogan: ads::slogan(Some(nd_sim::BusinessKind::FastFood), false, 1) }, ads::Ad { mark: marks[0].0, name: marks[0].1.clone(), slogan: ads::slogan(None, true, 2) }]];
    for (row, show) in shows.iter().enumerate() {
        let fr = ads::frames(show, 12.0, 5.0);
        for (k, (t, paint)) in fr.iter().enumerate() {
            let mut t = t.clone();
            // Screen metres into the unit square (12 m across = 2 units).
            t.place(1.0 / 6.0, Vec2::ZERO);
            let cell = w / fr.len().max(1);
            let paint = Paint { brand: half(paint.brand), deep: half(paint.deep), accent: half(paint.accent), light: half(paint.light), dark: paint.dark };
            logo::rasterize(&mut buf, w, k * cell + 4, CELL * rows + row * ad_h + (ad_h - cell) / 2, cell - 8, &t, &paint);
        }
    }
    write_png(&out, w, h, &buf);
    println!("wrote {out} ({} logos, {w}x{h})", marks.len());
}

fn half(c: [f32; 3]) -> [f32; 3] {
    [c[0] / 2.2, c[1] / 2.2, c[2] / 2.2]
}

/// Linear to sRGB bytes.
fn srgb(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let s = if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// An uncompressed (stored-deflate) RGB PNG.
fn write_png(path: &str, w: usize, h: usize, px: &[[f32; 3]]) {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let p = px[y * w + x];
            raw.extend_from_slice(&[srgb(p[0]), srgb(p[1]), srgb(p[2])]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        let n = chunk.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        png.extend_from_slice(&c);
        png.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::write(path, png).expect("write png");
}
