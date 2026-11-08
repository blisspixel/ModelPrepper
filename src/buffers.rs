//! Recorded buffers for one blocking transfer connection.
//!
//! ureq 3.4 is configured with a 128 KiB buffer on each side of the rustls
//! stream. rustls 0.23.46 caps received plaintext at 16 KiB and outgoing TLS
//! bytes at 64 KiB. The segment reader copies at most 64 KiB from that stream.
//! Kernel receive and send buffers are not those constants:
//! [`measure_kernel_socket_buffers`] reads `SO_RCVBUF` and `SO_SNDBUF` on a
//! connected loopback socket whose sizes were left unset, which is how ureq
//! opens its socket.
//!
//! A full GET larger than [`MAX_SINGLE_GET_BYTES`] is outside this path. The
//! Hub CDN returns 400 for one. Payload bytes are requested with `Range`.
//!
//! None of these figures is a plan `workspace_bytes`. That field stays null
//! until a later bound also covers every selected file.

use crate::{Error, Result};
use socket2::SockRef;
use std::{
    io::ErrorKind,
    net::{Ipv4Addr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

/// Bytes copied from the segment body into the staging file.
pub(crate) const APPLICATION_READ_BYTES: usize = 64 * 1024;
/// ureq input buffer around the TLS reader.
pub(crate) const TLS_INPUT_BYTES: usize = 128 * 1024;
/// ureq output buffer around the TLS writer.
pub(crate) const TLS_OUTPUT_BYTES: usize = 128 * 1024;
/// rustls 0.23.46 received-plaintext cap.
pub(crate) const TLS_RECEIVED_PLAINTEXT_BYTES: usize = 16 * 1024;
/// rustls 0.23.46 outgoing plaintext and TLS-record cap.
pub(crate) const TLS_SEND_BUFFER_BYTES: usize = 64 * 1024;
/// Decimal 50 GB. A full GET above this is outside the ranged payload path.
pub(crate) const MAX_SINGLE_GET_BYTES: u64 = 50_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedTransferBuffers {
    pub application_read_bytes: usize,
    pub tls_input_bytes: usize,
    pub tls_output_bytes: usize,
    pub tls_received_plaintext_bytes: usize,
    pub tls_send_buffer_bytes: usize,
    pub max_single_get_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelSocketBuffers {
    pub receive_bytes: u64,
    pub send_bytes: u64,
}

/// True when the HTTP agent is still using the recorded TLS buffer sizes.
pub fn http_agent_matches_recorded_buffers(http: &crate::http::Http) -> bool {
    let (input, output) = http.buffer_sizes();
    input == TLS_INPUT_BYTES && output == TLS_OUTPUT_BYTES
}

pub fn recorded_transfer_buffers() -> RecordedTransferBuffers {
    RecordedTransferBuffers {
        application_read_bytes: APPLICATION_READ_BYTES,
        tls_input_bytes: TLS_INPUT_BYTES,
        tls_output_bytes: TLS_OUTPUT_BYTES,
        tls_received_plaintext_bytes: TLS_RECEIVED_PLAINTEXT_BYTES,
        tls_send_buffer_bytes: TLS_SEND_BUFFER_BYTES,
        max_single_get_bytes: MAX_SINGLE_GET_BYTES,
    }
}

/// Read kernel socket buffers from a connected loopback TCP socket.
/// The hub is not contacted. Sizes are left unset, matching a ureq socket.
pub fn measure_kernel_socket_buffers() -> Result<KernelSocketBuffers> {
    read_socket_buffers(None)
}

fn read_socket_buffers(receive_request: Option<usize>) -> Result<KernelSocketBuffers> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(measure_error)?;
    listener.set_nonblocking(true).map_err(measure_error)?;
    let address = listener.local_addr().map_err(measure_error)?;
    let accepted = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return Ok(stream),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(error);
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    });
    let client = match TcpStream::connect(address) {
        Ok(client) => client,
        Err(error) => {
            let _ = accepted.join();
            return Err(measure_error(error));
        }
    };
    let socket = SockRef::from(&client);
    if let Some(size) = receive_request {
        socket.set_recv_buffer_size(size).map_err(measure_error)?;
    }
    let receive = socket.recv_buffer_size().map_err(measure_error)?;
    let send = socket.send_buffer_size().map_err(measure_error)?;
    match accepted.join() {
        Ok(Ok(server)) => drop(server),
        Ok(Err(error)) => return Err(measure_error(error)),
        Err(_) => {
            return Err(Error::new(
                "buffer_measurement_failed",
                "kernel socket buffers could not be read",
            ));
        }
    }
    Ok(KernelSocketBuffers {
        receive_bytes: receive as u64,
        send_bytes: send as u64,
    })
}

fn measure_error(error: std::io::Error) -> Error {
    Error::new(
        "buffer_measurement_failed",
        format!("kernel socket buffers could not be read ({})", error.kind()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_figures_match_the_inspected_stacks() {
        let recorded = recorded_transfer_buffers();
        assert_eq!(recorded.application_read_bytes, 64 * 1024);
        assert_eq!(recorded.tls_input_bytes, 128 * 1024);
        assert_eq!(recorded.tls_output_bytes, 128 * 1024);
        assert_eq!(recorded.tls_received_plaintext_bytes, 16 * 1024);
        assert_eq!(recorded.tls_send_buffer_bytes, 64 * 1024);
        assert_eq!(recorded.max_single_get_bytes, 50_000_000_000);
    }

    #[test]
    fn kernel_socket_buffers_are_read_from_the_socket() {
        let unset = measure_kernel_socket_buffers().unwrap();
        assert!(unset.receive_bytes > 0, "{unset:?}");
        assert!(unset.send_bytes > 0, "{unset:?}");
        let low = read_socket_buffers(Some(8 * 1024)).unwrap();
        let high = read_socket_buffers(Some(64 * 1024)).unwrap();
        assert!(
            high.receive_bytes > low.receive_bytes,
            "low {low:?} high {high:?} unset {unset:?}"
        );
    }
}
