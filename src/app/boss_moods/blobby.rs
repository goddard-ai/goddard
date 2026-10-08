//! Rust port of enjeck/blobby-svg, src/index.ts at
//! 9e462ab7cdacab9059e379a8da6fcec9e4e1bc05 (MIT; see blobby-LICENSE).
//! Each upstream Math.random draw uses its own stable FNV-1a/Mulberry32 key.
//! At <=24 logical pixels, outlines are widened to one logical pixel and
//! blush is omitted so facial features survive downsampling.
use super::{AVATAR_SOURCE_SIZE, js_round, num, value};
use std::fmt::Write as _;

const COLORS: [&str; 19] = [
    "#CEE5D0", "#ff8080", "#79B4B7", "#6B7AA1", "#DEBA9D", "#F6AE99", "#FFBCBC", "#B5EAEA",
    "#CEE5D0", "#c0dba9", "#b8e0b6", "#9A8194", "#d8db76", "#E8E9A1", "#ECB390", "#CFDAC8",
    "#f0c0c0", "#E5EDB7", "#F6DEF6",
];

pub(super) fn avatar_svg(seed: &str, bucket: u32) -> Vec<u8> {
    let draw = |key: &str| value(seed, &format!("blobby:{key}"));
    let size = js_round(draw("size") * 10.0 + 95.0);
    let growth = js_round(draw("growth") * 3.0 + 4.0);
    let edges = js_round(draw("edges") * 2.0 + 6.0) as usize;
    let outer = size / 2.0;
    let inner = growth * (outer / 10.0);
    let points: Vec<_> = (0..edges)
        .map(|i| {
            let mut radius = inner + (draw(&format!("point:{i}")) + 0.1) * (outer - inner);
            if radius > outer {
                radius -= inner;
            } else if radius < inner {
                radius += inner;
            }
            let angle = (i as f64 * 360.0 / edges as f64).floor().to_radians();
            (
                js_round(outer + radius * angle.cos()),
                js_round(outer + radius * angle.sin()),
            )
        })
        .collect();
    let mut path = String::with_capacity(256);
    let _ = write!(
        path,
        "M{},{}",
        num((points[0].0 + points[1].0) / 2.0),
        num((points[0].1 + points[1].1) / 2.0)
    );
    for i in 0..edges {
        let a = points[(i + 1) % edges];
        let b = points[(i + 2) % edges];
        let _ = write!(
            path,
            "Q{},{},{},{}",
            num(a.0),
            num(a.1),
            num((a.0 + b.0) / 2.0),
            num((a.1 + b.1) / 2.0)
        );
    }
    path.push('Z');
    let color = COLORS[(draw("color") * COLORS.len() as f64).floor() as usize];
    let stroke = if bucket <= 24 {
        100.0 / bucket.max(1) as f64
    } else {
        2.0
    };
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="{AVATAR_SOURCE_SIZE}" height="{AVATAR_SOURCE_SIZE}"><path fill="{color}" d="{path}"/>"##
    );
    if bucket > 24 {
        out.push_str(r##"<g fill="#fff" fill-opacity="0.4"><circle cx="70" cy="65" r="6"/><circle cx="30" cy="65" r="6"/></g>"##);
    }
    let _ = write!(
        out,
        r##"<path transform="translate(-3,-3)" stroke="#000" stroke-width="{}" fill="none" d="{path}"/>"##,
        num(stroke)
    );
    let eye_size = js_round(draw("eye:size") * 4.0 + 6.0);
    let x = draw("eye:x") * 4.0 - 2.0;
    let y = draw("eye:y") * 4.0 - 2.0;
    let single = (draw("eye:count") * 10.0).floor() < 5.0;
    let centers: &[f64] = if single { &[50.0] } else { &[38.0, 58.0] };
    for (i, center) in centers.iter().enumerate() {
        let pupil = if single {
            eye_size / 2.0
        } else {
            js_round(draw(&format!("eye:pupil:{i}")) * (eye_size / 3.0 - 3.0) + 3.0)
        };
        let _ = write!(
            out,
            r##"<g transform="translate({},50)"><circle r="{}" stroke="#000" stroke-width="{}" fill="#fff"/><circle cx="{}" cy="{}" r="{}" fill="#000"/></g>"##,
            num(*center),
            num(eye_size),
            num(stroke),
            num(x),
            num(y),
            num(pupil)
        );
    }
    out.push_str("</svg>");
    out.into_bytes()
}
