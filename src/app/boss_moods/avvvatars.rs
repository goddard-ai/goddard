//! Faithful nusu/avvvatars shape renderer, revision
//! 75bd2968a755d32be2116c2a70a65239659400ed. MIT + bundled MT19937 BSD notice
//! in avvvatars-LICENSE. Upstream has character/shape styles, no gradient.
use super::AVATAR_SOURCE_SIZE;
const SHAPES: &[&str] = include!("avvvatars_shapes.rs");
const BACKGROUND_COLORS: [&str; 20] = [
    "#F7F9FC", "#EEEDFD", "#FFEBEE", "#FDEFE2", "#E7F9F3", "#EDEEFD", "#ECFAFE", "#F2FFD1",
    "#FFF7E0", "#FDF1F7", "#EAEFE6", "#E0E6EB", "#E4E2F3", "#E6DFEC", "#E2F4E8", "#E6EBEF",
    "#EBE6EF", "#E8DEF6", "#D8E8F3", "#ECE1FE",
];
const SHAPE_COLORS: [&str; 20] = [
    "#060A23", "#5E36F5", "#E11234", "#E87917", "#3EA884", "#0618BC", "#0FBBE6", "#87B80A",
    "#FFC933", "#EE77AF", "#69785E", "#2D3A46", "#280F6D", "#37364F", "#363548", "#4D176E",
    "#AB133E", "#420790", "#222A54", "#192251",
];
// JavaScript ToUint32: Mash can exceed u32, so truncation must wrap.
fn uint32(n: f64) -> u32 {
    n.trunc().rem_euclid(4294967296.0) as u32
}
fn mash(n: &mut f64, input: &str) -> f64 {
    for unit in input.encode_utf16() {
        *n += f64::from(unit);
        let mut h = 0.02519603282416938 * *n;
        *n = f64::from(uint32(h));
        h -= *n;
        h *= *n;
        *n = f64::from(uint32(h));
        h -= *n;
        *n += h * 4294967296.0;
    }
    f64::from(uint32(*n)) / 4294967296.0
}
fn random(seed: &str) -> f64 {
    let mut n = 0xefc8249d_u32 as f64;
    mash(&mut n, " ");
    let mut s1 = mash(&mut n, " ");
    mash(&mut n, " ");
    mash(&mut n, seed);
    s1 -= mash(&mut n, seed);
    if s1 < 0.0 {
        s1 += 1.0;
    }
    // The upstream generator uses only the first MT19937 output.
    let mut mt = [0_u32; 624];
    mt[0] = uint32(s1 * 10000000.0);
    for i in 1..624 {
        mt[i] = 1812433253_u32
            .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
            .wrapping_add(i as u32);
    }
    let y = (mt[0] & 0x80000000) | (mt[1] & 0x7fffffff);
    let mut first = mt[397] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908b0df } else { 0 };
    first ^= first >> 11;
    first ^= (first << 7) & 0x9d2c5680;
    first ^= (first << 15) & 0xefc60000;
    first ^= first >> 18;
    f64::from(first) / 4294967296.0
}
pub(super) fn avatar_svg(seed: &str, bucket: u32) -> Vec<u8> {
    let r = random(seed);
    let color = (r * 20.0).floor() as usize;
    let shape = (r * 60.0).floor() as usize;
    // CSS upstream rounds half-size to an integer logical pixel.
    let extent = super::js_round(bucket as f64 * 0.5) / bucket.max(1) as f64 * 100.0;
    let offset = (100.0 - extent) / 2.0;
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="{AVATAR_SOURCE_SIZE}" height="{AVATAR_SOURCE_SIZE}"><circle cx="50" cy="50" r="50" fill="{}"/><svg x="{offset}" y="{offset}" width="{extent}" height="{extent}" viewBox="0 0 32 32" fill="none" color="{}">{}</svg></svg>"##, BACKGROUND_COLORS[color], SHAPE_COLORS[color], SHAPES[shape]).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_match_upstream() {
        for (seed, color, shape) in [
            ("test", 5, 18),
            ("Felix", 18, 56),
            ("", 6, 20),
            ("😀", 0, 2),
        ] {
            let r = random(seed);
            assert_eq!((r * 20.0).floor() as usize, color);
            assert_eq!((r * 60.0).floor() as usize + 1, shape);
        }
    }
}
