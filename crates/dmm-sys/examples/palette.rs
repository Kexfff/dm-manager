//! Prints the accent colors of images (decoded with ImageMagick): `palette <image>…`

fn main() {
    for path in std::env::args().skip(1) {
        let out = std::process::Command::new("magick").args([&path, "-resize", "96x96", "rgb:-"]).output().unwrap();
        let pixels: Vec<(u8, u8, u8)> = out.stdout.as_chunks::<3>().0.iter().map(|p| (p[0], p[1], p[2])).collect();
        let colors = dmm_sys::palette::accents(&pixels, 6);
        let hex: Vec<String> = colors.iter().map(|c| format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)).collect();
        println!("{path} {}", hex.join(" "));
    }
}
