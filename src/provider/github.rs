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
        let header = |name: &str| -> Result<&str, VerifyError> {
            let mut values = request.headers.get_all(name).iter();
            let first = values
                .next()
                .ok_or(VerifyError)?
                .to_str()
                .map_err(|_| VerifyError)?;
            if values.next().is_some() {
                return Err(VerifyError);
            }
            Ok(first)
        };
        let signature = header("x-hub-signature-256")?
            .strip_prefix("sha256=")
            .ok_or(VerifyError)?;
        if signature.len() != 64 {
            return Err(VerifyError);
        }
        let signature = hex::decode(signature).map_err(|_| VerifyError)?;
        let secret = config
            .get("secret")
            .and_then(toml::Value::as_str)
            .ok_or(VerifyError)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| VerifyError)?;
        mac.update(&request.body);
        mac.verify_slice(&signature).map_err(|_| VerifyError)?;
        let mime = header("content-type")?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !mime.eq_ignore_ascii_case("application/json") {
            return Err(VerifyError);
        }
        let json: serde_json::Value =
            serde_json::from_slice(&request.body).map_err(|_| VerifyError)?;
        if !json.is_object() {
            return Err(VerifyError);
        }
        let event = header("x-github-event")?;
        if event.is_empty()
            || event.len() > 128
            || !event
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(VerifyError);
        }
        let delivery = header("x-github-delivery")?;
        if uuid::Uuid::parse_str(delivery).is_err() {
            return Err(VerifyError);
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
