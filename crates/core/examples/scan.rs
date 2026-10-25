//! Prints the speakers found on the LAN, with serial and address masked (discovery already
//! masks the serial inside the announced name).

use neumann_control_core::discovery::{scan, SCAN_DURATION};
use neumann_control_core::mask::{mask_host, mask_serial};

const UNKNOWN: &str = "unknown";

fn main() {
    match scan(SCAN_DURATION) {
        Ok(speakers) => {
            println!("{} speaker(s) found", speakers.len());
            for speaker in speakers {
                let serial = speaker
                    .serial
                    .as_deref()
                    .map_or_else(|| UNKNOWN.to_string(), mask_serial);
                println!(
                    "{} | {} | {} | {}",
                    speaker.name,
                    speaker.product.as_deref().unwrap_or(UNKNOWN),
                    serial,
                    mask_host(&speaker.address)
                );
            }
        }
        Err(message) => eprintln!("scan failed: {message}"),
    }
}
