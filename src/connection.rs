use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct Config {
    pub url: String,
    pub credentials: Option<PathBuf>,
    pub ca: Option<PathBuf>,
    pub certificate: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub tls: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let path = |key| std::env::var_os(key).map(PathBuf::from);
        let config = Self {
            url: std::env::var("NATSUI_URL").unwrap_or("nats://127.0.0.1:4222".into()),
            credentials: path("NATSUI_CREDS"),
            ca: path("NATSUI_TLS_CA"),
            certificate: path("NATSUI_TLS_CERT"),
            key: path("NATSUI_TLS_KEY"),
            tls: match std::env::var("NATSUI_TLS_REQUIRED").as_deref() {
                Err(_) | Ok("0") => false,
                Ok("1") => true,
                _ => return Err("NATSUI_TLS_REQUIRED must be 0 or 1".into()),
            },
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| "Invalid NATSUI_URL")?;
        if !matches!(url.scheme(), "nats" | "tls")
            || url.host_str().is_none()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("NATSUI_URL must be one nats:// or tls:// server address".into());
        }
        if self.certificate.is_some() != self.key.is_some() {
            return Err("NATSUI_TLS_CERT and NATSUI_TLS_KEY must be provided together".into());
        }
        if self.credentials.is_some() && (!url.username().is_empty() || url.password().is_some()) {
            return Err("Use either NATSUI_CREDS or URL authentication, not both".into());
        }
        for (name, path) in [
            ("NATSUI_CREDS", &self.credentials),
            ("NATSUI_TLS_CA", &self.ca),
            ("NATSUI_TLS_CERT", &self.certificate),
            ("NATSUI_TLS_KEY", &self.key),
        ] {
            if let Some(path) = path {
                let metadata =
                    std::fs::metadata(path).map_err(|_| format!("{name} is not readable"))?;
                if !metadata.is_file() || metadata.len() > 1_048_576 {
                    return Err(format!("{name} must be a file smaller than 1 MiB"));
                }
                std::fs::File::open(path).map_err(|_| format!("{name} is not readable"))?;
            }
        }
        Ok(())
    }

    // Only a digest is stored. Credential rotation intentionally needs a new
    // profile when a stable public identity cannot be established from a URL.
    pub fn binding(&self, domain: &str) -> Result<String, String> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| "Invalid NATSUI_URL")?;
        let file_digest = |path: &Option<PathBuf>| -> Result<Option<String>, String> {
            path.as_ref()
                .map(|p| {
                    std::fs::read(p)
                        .map(|b| format!("{:x}", Sha256::digest(b)))
                        .map_err(|_| "Connection identity file is not readable".into())
                })
                .transpose()
        };
        let principal = if url.password().is_some() {
            url.username()
        } else {
            ""
        };
        let identity = json!({"host":url.host_str(),"port":url.port().unwrap_or(4222),
            "domain":domain,"user":principal,
            "token":if url.password().is_none() && !url.username().is_empty() {Some(format!("{:x}",Sha256::digest(url.username())))}else{None},
            "credentials":file_digest(&self.credentials)?,"client_certificate":file_digest(&self.certificate)?});
        Ok(format!("{:x}", Sha256::digest(identity.to_string())))
    }

    pub async fn options(&self) -> Result<async_nats::ConnectOptions, String> {
        let mut options = if let Some(path) = &self.credentials {
            async_nats::ConnectOptions::with_credentials_file(path)
                .await
                .map_err(|_| "NATSUI_CREDS could not be parsed")?
        } else {
            async_nats::ConnectOptions::new()
        };
        let url = reqwest::Url::parse(&self.url).map_err(|_| "Invalid NATSUI_URL")?;
        let decode = |value: &str| {
            percent_encoding::percent_decode_str(value)
                .decode_utf8()
                .map(|v| v.into_owned())
                .map_err(|_| "URL authentication must be valid UTF-8")
        };
        if let Some(password) = url.password() {
            options = options.user_and_password(decode(url.username())?, decode(password)?);
        } else if !url.username().is_empty() {
            options = options.token(decode(url.username())?);
        }
        if let Some(path) = &self.ca {
            options = options.add_root_certificates(path.clone());
        }
        if let (Some(cert), Some(key)) = (&self.certificate, &self.key) {
            options = options.add_client_certificate(cert.clone(), key.clone());
        }
        Ok(options
            .name("natsui / read-only")
            .require_tls(
                self.tls
                    || self.ca.is_some()
                    || self.certificate.is_some()
                    || self.url.starts_with("tls://"),
            )
            .connection_timeout(Duration::from_secs(3)))
    }

    pub async fn connect(&self) -> Result<async_nats::Client, String> {
        let options = self.options().await?;
        match tokio::time::timeout(Duration::from_secs(4), options.connect(&self.url)).await {
            Ok(Ok(client)) => Ok(client),
            Ok(Err(error)) => Err(format!(
                "NATS connection failed ({:?}). Check the address, credentials and TLS trust. Connection secrets are omitted.",
                error.kind()
            )),
            Err(_) => Err(
                "NATS connection timed out. Check network reachability and TLS configuration."
                    .into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn config(url: &str) -> Config {
        Config {
            url: url.into(),
            credentials: None,
            ca: None,
            certificate: None,
            key: None,
            tls: false,
        }
    }
    #[test]
    fn identity_guards_endpoint_domain_and_user_without_storing_secrets() {
        let a = config("nats://reader:secret@localhost:4222");
        let binding = a.binding("").unwrap();
        assert_eq!(
            binding,
            config("tls://reader:rotated@localhost:4222")
                .binding("")
                .unwrap()
        );
        assert_ne!(
            binding,
            config("nats://other:secret@localhost:4222")
                .binding("")
                .unwrap()
        );
        assert_ne!(binding, a.binding("OTHER").unwrap());
        assert_ne!(
            binding,
            config("nats://reader:secret@localhost:4223")
                .binding("")
                .unwrap()
        );
        assert_eq!(binding.len(), 64);
        assert!(!binding.contains("secret"));
        assert!(config("http://localhost").validate().is_err());
        let mut partial = a;
        partial.certificate = Some("missing.pem".into());
        assert!(partial.validate().unwrap_err().contains("together"));
    }
}
