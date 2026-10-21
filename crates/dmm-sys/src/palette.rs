//! Accent colors from an image (the wallpaper): its most prominent vivid hues, each made
//! bright and saturated enough to work as an accent on a dark interface.

pub type Rgb = (u8, u8, u8);

const BUCKETS: usize = 24; // 15° of hue each

/// Up to `count` accent colors, most prominent first. Grayscale images give none.
pub fn accents(pixels: &[Rgb], count: usize) -> Vec<Rgb> {
    // Vivid hues first; then quieter ones, if the picture has few vivid colors.
    let mut picked =
        hues(pixels, count, &Pass { min_saturation: 0.22, min_value: 0.2, min_share: 0.02, distance: 28.0 }, &[]);
    if picked.len() < count {
        let relaxed = Pass { min_saturation: 0.1, min_value: 0.12, min_share: 0.005, distance: 22.0 };
        picked = hues(pixels, count, &relaxed, &picked);
    }
    // A near-monochrome picture: offer lighter and deeper shades of what it has.
    if !picked.is_empty() && picked.len() < 3 {
        let base: Vec<Rgb> = picked.iter().map(|(_, c)| *c).collect();
        for c in base {
            let (h, s, _) = hsl(c);
            for l in [0.72, 0.42] {
                if picked.len() < count {
                    picked.push((h, from_hsl(h, s, l)));
                }
            }
        }
    }
    picked.into_iter().map(|(_, c)| c).collect()
}

struct Pass {
    min_saturation: f32,
    min_value: f32,
    /// A hue must cover at least this share of the colorful pixels.
    min_share: f32,
    /// Degrees between picked hues.
    distance: f32,
}

/// Adds the image's most prominent hues to `already`, as `(hue, accent color)`.
fn hues(pixels: &[Rgb], count: usize, pass: &Pass, already: &[(f32, Rgb)]) -> Vec<(f32, Rgb)> {
    let mut weight = [0f32; BUCKETS];
    let mut sum = [(0f32, 0f32, 0f32); BUCKETS];
    let mut total = 0f32;
    for &(r, g, b) in pixels {
        let (h, s, v) = hsv((r, g, b));
        // Grays and near-blacks say little about a picture's color.
        if s < pass.min_saturation || v < pass.min_value {
            continue;
        }
        // Area counts most; vivid, bright pixels a bit more than dull ones.
        let w = s * (0.5 + 0.5 * v);
        let i = ((h / 360.0 * BUCKETS as f32) as usize).min(BUCKETS - 1);
        weight[i] += w;
        sum[i].0 += f32::from(r) * w;
        sum[i].1 += f32::from(g) * w;
        sum[i].2 += f32::from(b) * w;
        total += w;
    }
    let mut picked = already.to_vec();
    if total == 0.0 {
        return picked;
    }
    let mut order: Vec<usize> = (0..BUCKETS).filter(|&i| weight[i] > total * pass.min_share).collect();
    order.sort_by(|a, b| weight[*b].total_cmp(&weight[*a]));
    for i in order {
        if picked.len() >= count {
            break;
        }
        let w = weight[i];
        let average = ((sum[i].0 / w) as u8, (sum[i].1 / w) as u8, (sum[i].2 / w) as u8);
        let color = accentize(average);
        let hue = hsv(color).0;
        if picked.iter().all(|(h, _)| hue_distance(*h, hue) >= pass.distance) {
            picked.push((hue, color));
        }
    }
    picked
}

/// Brings a color into accent range: clearly saturated, mid-light.
fn accentize(c: Rgb) -> Rgb {
    let (h, s, l) = hsl(c);
    from_hsl(h, s.clamp(0.55, 0.9), l.clamp(0.5, 0.66))
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

fn hsv((r, g, b): Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let s = if max == 0.0 { 0.0 } else { d / max };
    (hue(r, g, b, max, d), s, max)
}

fn hsl((r, g, b): Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.0;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    (hue(r, g, b, max, d), s, l)
}

fn hue(r: f32, g: f32, b: f32, max: f32, d: f32) -> f32 {
    if d == 0.0 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

fn from_hsl(h: f32, s: f32, l: f32) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (byte(r), byte(g), byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_main_hues_in_order() {
        // Mostly dark blue, some orange, a little gray.
        let mut pixels = vec![(20, 40, 120); 600];
        pixels.extend(vec![(230, 120, 30); 300]);
        pixels.extend(vec![(128, 128, 128); 400]);
        let colors = accents(&pixels, 5);
        assert!(colors.len() >= 2, "{colors:?}");
        let hues: Vec<f32> = colors.iter().map(|c| hsv(*c).0).collect();
        assert!((hues[0] - 228.0).abs() < 8.0, "{hues:?}");
        assert!((hues[1] - 28.0).abs() < 8.0, "{hues:?}");
        // The picked hues are bright enough to read on a dark background.
        assert!(colors[..2].iter().all(|c| hsl(*c).2 >= 0.49));
    }

    #[test]
    fn grayscale_gives_nothing() {
        assert!(accents(&[(10, 10, 10), (200, 200, 200), (90, 92, 91)], 5).is_empty());
    }

    #[test]
    fn similar_hues_merge_and_get_shades() {
        let mut pixels = vec![(200, 30, 30); 500];
        pixels.extend(vec![(200, 50, 30); 500]);
        let colors = accents(&pixels, 5);
        // One red, plus a lighter and a deeper one.
        assert_eq!(colors.len(), 3, "{colors:?}");
        assert!(colors.iter().all(|c| hue_distance(hsv(*c).0, hsv(colors[0]).0) < 1.0));
    }

    #[test]
    fn hsl_round_trip() {
        for c in [(233, 84, 32), (61, 174, 233), (0, 211, 184)] {
            let (h, s, l) = hsl(c);
            let back = from_hsl(h, s, l);
            assert!((i32::from(back.0) - i32::from(c.0)).abs() <= 1 && (i32::from(back.2) - i32::from(c.2)).abs() <= 1);
        }
    }
}
