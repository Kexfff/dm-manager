//! Prints what the Overview page shows (read-only).

fn main() {
    let info = dmm_sys::sysinfo::read();
    println!("{info:#?}\n\n{}", info.summary());
}
