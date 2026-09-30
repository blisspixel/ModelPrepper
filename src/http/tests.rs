use super::*;
use crate::fixture::{Server, response};

#[test]
fn redirect_trust_is_exact_and_credentials_are_never_accepted() {
    for allowed in [
        "https://huggingface.co/a",
        "https://cdn-lfs.hf.co/a?signature=private",
        "https://cas-bridge.xethub.hf.co/a",
        "https://huggingface.co:443/a",
    ] {
        validate_url(&Url::parse(allowed).unwrap(), None).unwrap();
    }
    for denied in [
        "http://huggingface.co/a",
        "https://huggingface.co.evil/a",
        "https://evilhf.co/a",
        "https://hf.co:444/a",
        "https://user@hf.co/a",
        "https://user:pass@hf.co/a",
        "https://hf.co/a#fragment",
        "file:///a",
        "https://127.0.0.1/a",
    ] {
        assert_eq!(
            validate_url(&Url::parse(denied).unwrap(), None)
                .unwrap_err()
                .code,
            "untrusted_url",
            "{denied}"
        );
    }
    validate_url(
        &Url::parse("http://127.0.0.1:123/a").unwrap(),
        Some("http://127.0.0.1:123"),
    )
    .unwrap();
}

#[test]
fn bounded_http_follows_only_allowed_redirects_and_preserves_exact_bytes() {
    let server = Server::new(vec![
        response(302, "Location: /pinned\r\n", b""),
        response(200, "Content-Encoding: identity\r\n", b"abc"),
    ]);
    assert_eq!(
        Http::fixture(&server.origin)
            .get(&Url::parse(&server.origin).unwrap(), 3)
            .unwrap(),
        b"abc"
    );
    let requests = server.finish();
    assert!(requests[1].starts_with("GET /pinned "));
    assert!(
        requests[0]
            .to_ascii_lowercase()
            .contains("accept-encoding: identity")
    );
}

#[test]
fn http_errors_do_not_disclose_signed_urls() {
    for (reply, limit, code) in [
        (response(403, "", b"secret"), 10, "source_status"),
        (response(200, "", b"abcd"), 3, "metadata_too_large"),
        (
            response(200, "Content-Encoding: gzip\r\n", b"abc"),
            3,
            "unexpected_encoding",
        ),
        (response(302, "", b""), 3, "invalid_redirect"),
        (
            response(302, "Location: https://evil.example/?secret=token\r\n", b""),
            3,
            "untrusted_url",
        ),
        (
            response(302, "Location: http://[\r\n", b""),
            3,
            "invalid_redirect",
        ),
        (response(200, "", b""), u64::MAX, "invalid_limit"),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\na".to_vec(),
            20,
            "source_read_failed",
        ),
    ] {
        let server = Server::new(vec![reply]);
        let url = Url::parse(&format!("{}/?secret=token", server.origin)).unwrap();
        let error = Http::fixture(&server.origin).get(&url, limit).unwrap_err();
        assert_eq!(error.code, code);
        assert!(!error.message.contains("token"));
        server.finish();
    }
    let server = Server::new(vec![response(302, "Location: /again\r\n", b""); 6]);
    assert_eq!(
        Http::fixture(&server.origin)
            .get(&Url::parse(&server.origin).unwrap(), 2)
            .unwrap_err()
            .code,
        "redirect_limit"
    );
    server.finish();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    assert_eq!(
        Http::fixture(&origin)
            .get(&Url::parse(&origin).unwrap(), 2)
            .unwrap_err()
            .code,
        "source_unavailable"
    );
}
