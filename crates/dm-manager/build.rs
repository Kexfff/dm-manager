//! Bundles the stylesheet, every icon in data/icons and the app icon into a GResource.

use std::fmt::Write;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut icons: Vec<String> = std::fs::read_dir("data/icons/scalable/actions")
        .expect("data/icons")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".svg"))
        .collect();
    icons.sort();

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gresources>\n  <gresource prefix=\"/io/github/kexfff/DmManager\">\n    <file>style.css</file>\n",
    );
    writeln!(
        xml,
        "    <file alias=\"icons/scalable/apps/io.github.kexfff.DmManager.svg\">../../../dist/io.github.kexfff.DmManager.svg</file>"
    )
    .unwrap();
    for icon in icons {
        writeln!(xml, "    <file preprocess=\"xml-stripblanks\">icons/scalable/actions/{icon}</file>").unwrap();
    }
    xml.push_str("  </gresource>\n</gresources>\n");
    let manifest = out.join("resources.gresource.xml");
    std::fs::write(&manifest, xml).unwrap();

    println!("cargo::rerun-if-changed=data");
    println!("cargo::rerun-if-changed=../../dist/io.github.kexfff.DmManager.svg");
    glib_build_tools::compile_resources(&["data"], manifest.to_str().unwrap(), "resources.gresource");
}
