use super::*;
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub struct GitHub;

impl WebhookProvider for GitHub {
    fn kind(&self) -> &'static str {
        "github"
    }

    fn configure(&self, ui: &mut dyn SetupUi) -> anyhow::Result<toml::Table> {
        Ok(toml::Table::from_iter([(
            "secret".into(),
            ui.secret("GitHub webhook secret")?.into(),
        )]))
    }

    fn validate_config(&self, config: &toml::Table) -> anyhow::Result<()> {
        anyhow::ensure!(
            config.len() == 1
                && config
                    .get("secret")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|s| !s.is_empty()),
            "github requires a nonempty secret and no unknown verification fields"
        );
        Ok(())
    }

    fn verify(
        &self,
        config: &toml::Table,
        request: &WebhookRequest,
    ) -> Result<VerificationOutcome, VerifyError> {
        let header = |name: &'static str| -> Result<&str, VerifyError> {
            let mut values = request.headers.get_all(name).iter();
            let first = values
                .next()
                .ok_or(VerifyError::new("missing_header").with_field(name))?
                .to_str()
                .map_err(|_| VerifyError::new("invalid_header").with_field(name))?;
            if values.next().is_some() {
                return Err(VerifyError::new("duplicate_header").with_field(name));
            }
            Ok(first)
        };
        let signature = header("x-hub-signature-256")?
            .strip_prefix("sha256=")
            .ok_or(VerifyError::new("invalid_signature").with_field("x-hub-signature-256"))?;
        if signature.len() != 64 {
            return Err(VerifyError::new("invalid_signature").with_field("x-hub-signature-256"));
        }
        let signature = hex::decode(signature)
            .map_err(|_| VerifyError::new("invalid_signature").with_field("x-hub-signature-256"))?;
        let secret = config
            .get("secret")
            .and_then(toml::Value::as_str)
            .ok_or(VerifyError::new("invalid_configuration").with_field("secret"))?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|_| VerifyError::new("invalid_configuration").with_field("secret"))?;
        mac.update(&request.body);
        mac.verify_slice(&signature).map_err(|_| {
            VerifyError::new("signature_mismatch").with_field("x-hub-signature-256")
        })?;
        let mime = header("content-type")?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !mime.eq_ignore_ascii_case("application/json") {
            return Err(VerifyError::new("unsupported_content_type").with_field("content-type"));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&request.body).map_err(|_| VerifyError::new("invalid_json"))?;
        if !json.is_object() {
            return Err(VerifyError::new("non_object_json"));
        }
        let event = header("x-github-event")?;
        if event.is_empty()
            || event.len() > 128
            || !event
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(VerifyError::new("invalid_header").with_field("x-github-event"));
        }
        let delivery = header("x-github-delivery")?;
        if uuid::Uuid::parse_str(delivery).is_err() {
            return Err(VerifyError::new("invalid_header").with_field("x-github-delivery"));
        }
        if event == "ping" {
            return Ok(VerificationOutcome::Respond {
                status: StatusCode::OK,
                body: Bytes::new(),
            });
        }
        Ok(VerificationOutcome::Dispatch(VerifiedPayload {
            body: request.body.clone(),
            content_type: "application/json".into(),
            event: event.to_owned(),
            delivery_id: delivery.to_owned(),
        }))
    }
}
