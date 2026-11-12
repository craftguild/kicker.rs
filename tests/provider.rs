mod common;
use common::*;
use kicker::provider::{Providers, VerificationOutcome};

#[test]
fn hmac_matches_github_official_vector() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(b"It's a Secret to Everybody").unwrap();
    mac.update(b"Hello, World!");
    assert_eq!(
        hex::encode(mac.finalize().into_bytes()),
        "757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17"
    );
}

#[test]
fn github_verifies_original_unicode_bytes_and_treats_ping_as_ack_only() {
    let registry = Providers::builtins();
    let provider = registry.get("github").unwrap();
    let config = hook(std::path::Path::new("/tmp"), "true").verification;
    let raw = "{ \"message\": \"日本語 🐥\" }\r\n".as_bytes();
    let VerificationOutcome::Dispatch(payload) =
        provider.verify(&config, &input(raw, "release")).unwrap()
    else {
        panic!("dispatch expected")
    };
    assert_eq!(payload.body, raw);
    assert_eq!(payload.event, "release");
    assert_eq!(payload.delivery_id, DELIVERY);
    assert!(matches!(
        provider.verify(&config, &input(raw, "ping")).unwrap(),
        VerificationOutcome::Respond {
            status: axum::http::StatusCode::OK,
            ..
        }
    ));
    assert!(provider.validate_config(&toml::Table::new()).is_err());
}

#[test]
fn missing_duplicate_malformed_and_tampered_deliveries_are_rejected() {
    let registry = Providers::builtins();
    let provider = registry.get("github").unwrap();
    let config = hook(std::path::Path::new("/tmp"), "true").verification;
    for name in [
        "x-hub-signature-256",
        "x-github-event",
        "x-github-delivery",
        "content-type",
    ] {
        let mut value = input(b"{}", "push");
        value.headers.remove(name);
        assert!(provider.verify(&config, &value).is_err(), "missing {name}");
        let mut value = input(b"{}", "push");
        value.headers.append(name, "duplicate".parse().unwrap());
        assert!(
            provider.verify(&config, &value).is_err(),
            "duplicate {name}"
        );
    }
    for (name, value) in [
        ("x-hub-signature-256", "sha1=bad"),
        ("x-hub-signature-256", "sha256=zz"),
        ("x-github-event", ""),
        ("x-github-event", "bad event"),
        ("x-github-delivery", "not-a-guid"),
        ("content-type", "application/x-www-form-urlencoded"),
    ] {
        let mut request = input(b"{}", "push");
        request.headers.insert(name, value.parse().unwrap());
        assert!(provider.verify(&config, &request).is_err());
    }
    let mut changed = input(b"{}", "push");
    changed.body = bytes::Bytes::from_static(b"{ }");
    assert!(provider.verify(&config, &changed).is_err());
    for raw in [b"not JSON".as_slice(), b"[]", b"null", &[0xff]] {
        assert!(provider.verify(&config, &input(raw, "push")).is_err());
    }
}
