//! Print the UPS report (the same one the app's "Create UPS report…" makes):
//! every connected HID UPS with its decoded values and full report layout.
//!
//!   cargo run --release -p ups-core --features win32 --example hiddump

fn main() {
    print!("{}", ups_core::report::generate(env!("CARGO_PKG_VERSION"), "", ""));
}
