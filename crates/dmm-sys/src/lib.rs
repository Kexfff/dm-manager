//! System backends for dm-manager: NetworkManager and BlueZ over D-Bus.
//!
//! No GTK here. Each backend is an async `run` loop that takes requests from a channel and
//! reports state through a callback, so the UI only ever sees plain data.

pub mod bluetooth;
mod bus;
pub mod network;

pub use bus::describe_error;
