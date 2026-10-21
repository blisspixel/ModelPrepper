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
        (response(404, "", b"missing"), 10, "source_status"),
        (response(401, "", b"nope"), 10, "source_status"),
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

fn assert_no_credentials(requests: &[String]) {
    for request in requests {
        let lower = request.to_ascii_lowercase();
        assert!(!lower.contains("authorization:"));
        assert!(!lower.contains("cookie:"));
    }
}

#[test]
fn expired_redirect_locations_are_refreshed_from_the_original_url_once() {
    let path = format!("/owner/model/resolve/{}/model.safetensors", "a".repeat(40));
    for rejected in [403u16, 401] {
        let server = Server::new(vec![
            response(302, "Location: /cdn?sig=old-secret\r\n", b""),
            response(rejected, "", b"sig=old-secret"),
            response(302, "Location: /cdn?sig=new-secret\r\n", b""),
            response(200, "", b"abc"),
        ]);
        let url = Url::parse(&format!("{}{path}", server.origin)).unwrap();
        assert_eq!(Http::fixture(&server.origin).get(&url, 3).unwrap(), b"abc");
        let requests = server.finish();
        assert_eq!(requests.len(), 4);
        assert!(requests[0].contains(&path));
        assert!(requests[1].contains("GET /cdn?sig=old-secret "));
        assert!(requests[2].contains(&path));
        assert!(requests[3].contains("GET /cdn?sig=new-secret "));
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.contains("sig=old-secret"))
                .count(),
            1
        );
        assert_no_credentials(&requests);
    }

    let server = Server::new(vec![
        response(302, "Location: /cdn?sig=old-secret\r\n", b""),
        response(403, "", b"sig=old-secret"),
        response(302, "Location: /cdn?sig=new-secret\r\n", b""),
        response(403, "", b"sig=new-secret"),
    ]);
    let url = Url::parse(&format!("{}{path}", server.origin)).unwrap();
    let error = Http::fixture(&server.origin).get(&url, 3).unwrap_err();
    assert_eq!(error.code, "source_status");
    assert!(!error.message.contains("sig"));
    let requests = server.finish();
    assert_eq!(requests.iter().filter(|r| r.contains(&path)).count(), 2);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains("sig=old-secret"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains("sig=new-secret"))
            .count(),
        1
    );
    assert_no_credentials(&requests);

    let server = Server::new(vec![response(403, "", b"sig=direct-secret")]);
    let url = Url::parse(&format!("{}{path}?sig=direct-secret", server.origin)).unwrap();
    let error = Http::fixture(&server.origin).get(&url, 3).unwrap_err();
    assert_eq!(error.code, "source_status");
    assert!(!error.message.contains("sig"));
    assert_eq!(server.finish().len(), 1);

    let server = Server::new(vec![
        response(302, "Location: /cdn?sig=old-secret\r\n", b""),
        response(403, "", b"sig=old-secret"),
        response(
            302,
            "Location: https://evil.example/?sig=evil-secret\r\n",
            b"",
        ),
    ]);
    let url = Url::parse(&format!("{}{path}", server.origin)).unwrap();
    let error = Http::fixture(&server.origin).get(&url, 3).unwrap_err();
    assert_eq!(error.code, "untrusted_url");
    assert!(!error.message.contains("sig"));
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| !request.contains("evil.example"))
    );
    assert_no_credentials(&requests);

    let mut replies = vec![
        response(302, "Location: /cdn?sig=old-secret\r\n", b""),
        response(401, "", b"sig=old-secret"),
    ];
    replies.extend(vec![
        response(
            302,
            "Location: /again?sig=still-secret\r\n",
            b""
        );
        6
    ]);
    let server = Server::new(replies);
    let url = Url::parse(&format!("{}{path}", server.origin)).unwrap();
    let error = Http::fixture(&server.origin).get(&url, 3).unwrap_err();
    assert_eq!(error.code, "redirect_limit");
    assert!(!error.message.contains("sig"));
    assert_eq!(server.finish().len(), 8);
}
