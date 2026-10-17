//! Prints the current Wi-Fi and Bluetooth state once (read-only).

use dmm_sys::{bluetooth, network};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (_ntx, nrx) = tokio::sync::mpsc::unbounded_channel();
    let (_btx, brx) = tokio::sync::mpsc::unbounded_channel();
    let (out, mut events) = tokio::sync::mpsc::unbounded_channel::<String>();
    let o = out.clone();
    tokio::spawn(network::run(nrx, move |e| {
        let _ = o.send(format!("{e:#?}"));
    }));
    tokio::spawn(bluetooth::run(brx, move |e| {
        let _ = out.send(format!("{e:#?}"));
    }));
    for _ in 0..2 {
        if let Some(e) = events.recv().await {
            println!("{e}");
        }
    }
}
