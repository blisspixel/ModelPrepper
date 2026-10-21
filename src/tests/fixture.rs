use std::{
    io::{Read, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// One plaintext HTTP exchange. Counts are bytes read and written on the
/// fixture socket. They are not TLS records or kernel socket-buffer sizes.
pub struct Exchange {
    pub request: String,
    pub read_bytes: u64,
    pub written_bytes: u64,
}

pub struct Server {
    pub origin: String,
    worker: JoinHandle<Vec<Exchange>>,
}

impl Server {
    pub fn new(responses: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let mut exchanges = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "fixture request timed out");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fixture accept: {error}"),
                    }
                };
                // macOS can inherit nonblocking mode from the listener.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                let read_bytes = request.len() as u64;
                let mut written_bytes = 0u64;
                let mut offset = 0;
                while offset < response.len() {
                    // A peer that hits its read limit can close before this write finishes.
                    match stream.write(&response[offset..]) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            written_bytes += count as u64;
                            offset += count;
                        }
                    }
                }
                exchanges.push(Exchange {
                    request: String::from_utf8(request).unwrap(),
                    read_bytes,
                    written_bytes,
                });
            }
            exchanges
        });
        Self { origin, worker }
    }

    pub fn finish_measured(self) -> Vec<Exchange> {
        self.worker.join().unwrap()
    }

    pub fn finish(self) -> Vec<String> {
        self.finish_measured()
            .into_iter()
            .map(|exchange| exchange.request)
            .collect()
    }
}

pub fn response(status: u16, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut result = format!(
        "HTTP/1.1 {status} Fixture\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n",
        body.len()
    )
    .into_bytes();
    result.extend(body);
    result
}
