mod common;
use common::*;
use kicker::provider::{Providers, VerificationOutcome, VerifyError};

fn assert_rejection(error: VerifyError, reason: &str, field: Option<&str>) {
    assert_eq!(error.reason(), reason);
    assert_eq!(error.field(), field);
    assert_eq!(error.to_string(), "invalid webhook");
}

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
        assert_rejection(
            provider.verify(&config, &value).err().unwrap(),
            "missing_header",
            Some(name),
        );
        let mut value = input(b"{}", "push");
        value.headers.append(name, "duplicate".parse().unwrap());
        assert_rejection(
            provider.verify(&config, &value).err().unwrap(),
            "duplicate_header",
            Some(name),
        );
        let mut value = input(b"{}", "push");
        value
            .headers
            .insert(name, axum::http::HeaderValue::from_bytes(&[0xff]).unwrap());
        assert_rejection(
            provider.verify(&config, &value).err().unwrap(),
            "invalid_header",
            Some(name),
        );
    }
    for (name, value, reason) in [
        ("x-hub-signature-256", "sha1=bad", "invalid_signature"),
        ("x-hub-signature-256", "sha256=zz", "invalid_signature"),
        ("x-github-event", "", "invalid_header"),
        ("x-github-event", "bad event", "invalid_header"),
        ("x-github-delivery", "not-a-guid", "invalid_header"),
        (
            "content-type",
            "application/x-www-form-urlencoded",
            "unsupported_content_type",
        ),
    ] {
        let mut request = input(b"{}", "push");
        request.headers.insert(name, value.parse().unwrap());
        assert_rejection(
            provider.verify(&config, &request).err().unwrap(),
            reason,
            Some(name),
        );
    }
    let mut bad_hex = input(b"{}", "push");
    bad_hex.headers.insert(
        "x-hub-signature-256",
        format!("sha256={}", "z".repeat(64)).parse().unwrap(),
    );
    assert_rejection(
        provider.verify(&config, &bad_hex).err().unwrap(),
        "invalid_signature",
        Some("x-hub-signature-256"),
    );
    let mut changed = input(b"{}", "push");
    changed.body = bytes::Bytes::from_static(b"{ }");
    changed.headers.remove("x-github-delivery");
    assert_rejection(
        provider.verify(&config, &changed).err().unwrap(),
        "signature_mismatch",
        Some("x-hub-signature-256"),
    );
    for (raw, reason) in [
        (b"not JSON".as_slice(), "invalid_json"),
        (b"[]".as_slice(), "non_object_json"),
        (b"null".as_slice(), "non_object_json"),
        (&[0xff], "invalid_json"),
    ] {
        assert_rejection(
            provider.verify(&config, &input(raw, "push")).err().unwrap(),
            reason,
            None,
        );
    }
    assert_rejection(
        provider
            .verify(&toml::Table::new(), &input(b"{}", "push"))
            .err()
            .unwrap(),
        "invalid_configuration",
        Some("secret"),
    );
}
