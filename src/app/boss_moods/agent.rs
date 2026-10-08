//! Agent Avatars Rust renderer, revision f1728b8d04bb52980714b4f5385c5784ff115e28.
//! MIT (agent-LICENSE). Standard light palette/catalog, default namespace,
//! raw seed mode (preserves the app's seed exactly, without normalization).
use super::AVATAR_SOURCE_SIZE;
use std::fmt::Write as _;
const ROWS: &[[u8; 4]] = include!("agent_rows.rs");
const PALETTES: [(&str, &str); 16] = [
    ("#F1DADA", "#492727"),
    ("#BCF1BC", "#274927"),
    ("#BCBCF1", "#272749"),
    ("#BCF1F1", "#274949"),
    ("#F1E8BC", "#494327"),
    ("#F1BCDF", "#49273E"),
    ("#C8DAF3", "#273549"),
    ("#EAF5E5", "#324927"),
    ("#F1BCBC", "#492727"),
    ("#C0EDD2", "#274935"),
    ("#EDD7F4", "#412749"),
    ("#F1D2BC", "#493527"),
    ("#E5EEF5", "#273B49"),
    ("#DFBCF1", "#3E2749"),
    ("#D6EDC0", "#384927"),
    ("#E9C4D3", "#492735"),
];
fn index(seed: &str, domain: &str, length: usize) -> usize {
    let message = format!(
        "deterministic-agent-avatars\0{version}\0{domain}\x007:default\0{len}:{seed}\x000",
        version = 1,
        len = seed.encode_utf16().count()
    );
    let mut h = 2166136261_u32;
    for byte in message.bytes() {
        h = (h ^ u32::from(byte)).wrapping_mul(16777619);
    }
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    if h == 0 {
        h = 0x9e3779b9;
    }
    let limit = 4294967296_u64 - 4294967296_u64 % length as u64;
    loop {
        h ^= h << 13;
        h ^= h >> 17;
        h ^= h << 5;
        if u64::from(h) < limit {
            return h as usize % length;
        }
    }
}
fn cell(rows: &[u8; 4], x: i32, y: i32) -> bool {
    (0..5).contains(&x) && (0..4).contains(&y) && rows[y as usize] & (1 << (4 - x)) != 0
}
pub(super) fn avatar_svg(seed: &str) -> Vec<u8> {
    let rows = &ROWS[index(seed, "shape", ROWS.len())];
    let (background, foreground) = PALETTES[index(seed, "palette", PALETTES.len())];
    let mut path = String::with_capacity(2048);
    for y in 0..4 {
        for x in 0..5 {
            if !cell(rows, x, y) {
                continue;
            }
            let up = cell(rows, x, y - 1);
            let down = cell(rows, x, y + 1);
            let left = cell(rows, x - 1, y);
            let right = cell(rows, x + 1, y);
            let (tl, tr, br, bl) = (
                (!up && !left),
                (!up && !right),
                (!down && !right),
                (!down && !left),
            );
            let (x, y) = (39 + x * 10, 44 + y * 10);
            let _ = write!(
                path,
                "M{} {y}H{}",
                x + if tl { 2 } else { 0 },
                x + 10 - if tr { 2 } else { 0 }
            );
            if tr {
                let _ = write!(path, "Q{} {y} {} {}", x + 10, x + 10, y + 2);
            } else {
                let _ = write!(path, "L{} {y}", x + 10);
            }
            let _ = write!(path, "V{}", y + 10 - if br { 2 } else { 0 });
            if br {
                let _ = write!(path, "Q{} {} {} {}", x + 10, y + 10, x + 8, y + 10);
            } else {
                let _ = write!(path, "L{} {}", x + 10, y + 10);
            }
            let _ = write!(path, "H{}", x + if bl { 2 } else { 0 });
            if bl {
                let _ = write!(path, "Q{x} {} {x} {}", y + 10, y + 8);
            } else {
                let _ = write!(path, "L{x} {}", y + 10);
            }
            let _ = write!(path, "V{}", y + if tl { 2 } else { 0 });
            if tl {
                let _ = write!(path, "Q{x} {y} {} {y}", x + 2);
            } else {
                let _ = write!(path, "L{x} {y}");
            }
            path.push('Z');
        }
    }
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="{AVATAR_SOURCE_SIZE}" height="{AVATAR_SOURCE_SIZE}"><circle cx="64" cy="64" r="48" fill="{background}"/><path d="{path}" fill="{foreground}" shape-rendering="geometricPrecision"/></svg>"##).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_match_upstream() {
        for (seed, shape, palette) in [
            ("test", 584, 14),
            ("Felix", 789, 10),
            ("", 377, 12),
            ("😀", 677, 14),
        ] {
            assert_eq!(index(seed, "shape", ROWS.len()), shape);
            assert_eq!(index(seed, "palette", PALETTES.len()), palette);
        }
    }
}
