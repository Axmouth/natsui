use futures_util::{StreamExt, stream::FuturesUnordered};
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

    fn urls(&self) -> Result<Vec<reqwest::Url>, String> {
        let entries: Vec<_> = self.url.split(',').map(str::trim).collect();
        if entries.is_empty() || entries.len() > 32 || entries.iter().any(|s| s.is_empty()) {
            return Err("NATSUI_URL requires 1 to 32 comma-separated server addresses without empty entries".into());
        }
        entries
            .into_iter()
            .map(|entry| {
                let url =
                    reqwest::Url::parse(entry).map_err(|_| "Invalid NATSUI_URL server address")?;
                if !matches!(url.scheme(), "nats" | "tls")
                    || url.host_str().is_none()
                    || !matches!(url.path(), "" | "/")
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    return Err(
                        "NATSUI_URL entries must be nats:// or tls:// server addresses".into(),
                    );
                }
                Ok(url)
            })
            .collect()
    }

    // One connection carries one account identity across all configured and discovered peers.
    fn auth_url(urls: &[reqwest::Url]) -> Result<&reqwest::Url, String> {
        let mut selected: Option<&reqwest::Url> = None;
        for url in urls
            .iter()
            .filter(|u| !u.username().is_empty() || u.password().is_some())
        {
            if selected.is_some_and(|previous| {
                previous.username() != url.username() || previous.password() != url.password()
            }) {
                return Err("All NATSUI_URL credentials must match. URL authentication is shared across the server list".into());
            }
            selected = Some(url);
        }
        Ok(selected.unwrap_or(&urls[0]))
    }

    pub fn validate(&self) -> Result<(), String> {
        let urls = self.urls()?;
        let auth = Self::auth_url(&urls)?;
        if self.certificate.is_some() != self.key.is_some() {
            return Err("NATSUI_TLS_CERT and NATSUI_TLS_KEY must be provided together".into());
        }
        if self.credentials.is_some() && (!auth.username().is_empty() || auth.password().is_some())
        {
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
        let urls = self.urls()?;
        let url = Self::auth_url(&urls)?;
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
        let mut identity = json!({"host":url.host_str(),"port":url.port().unwrap_or(4222),
            "domain":domain,"user":principal,
            "token":if url.password().is_none() && !url.username().is_empty() {Some(format!("{:x}",Sha256::digest(url.username())))}else{None},
            "credentials":file_digest(&self.credentials)?,"client_certificate":file_digest(&self.certificate)?});
        let mut endpoints: Vec<_> = urls
            .iter()
            .map(|u| (u.host_str().unwrap().to_owned(), u.port().unwrap_or(4222)))
            .collect();
        endpoints.sort();
        endpoints.dedup();
        // Preserve the existing single-server binding. Multi-server identity is independent
        // of list order and the winning connection, but changes when the configured set changes.
        if endpoints.len() > 1 {
            let object = identity.as_object_mut().unwrap();
            object.remove("host");
            object.remove("port");
            object.insert("servers".into(), json!(endpoints));
        }
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
        let urls = self.urls()?;
        let url = Self::auth_url(&urls)?;
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
                    || urls.iter().any(|u| u.scheme() == "tls"),
            )
            .connection_timeout(Duration::from_secs(3)))
    }

    pub async fn connect(&self) -> Result<async_nats::Client, String> {
        self.validate()?;
        let mut servers = self.urls()?;
        for url in &mut servers {
            // Credentials are applied through shared options, never exposed in server diagnostics.
            url.set_password(None)
                .map_err(|_| "Invalid NATS server address")?;
            url.set_username("")
                .map_err(|_| "Invalid NATS server address")?;
        }
        servers.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        servers.dedup();
        let servers: Vec<String> = servers.into_iter().map(|u| u.to_string()).collect();
        let race = async {
            let mut attempts = FuturesUnordered::new();
            for first in 0..servers.len() {
                let mut candidates = servers.clone();
                candidates.rotate_left(first);
                // Each attempt starts at a different seed and keeps the complete seed set
                // for later reconnects, even when the server advertises no peers.
                attempts.push(async move {
                    let options = self.options().await?.retain_servers_order();
                    options.connect(candidates).await.map_err(|error| format!(
                        "NATS connection failed ({:?}). Check the addresses, credentials and TLS trust. Connection secrets are omitted.",
                        error.kind()
                    ))
                });
            }
            let mut last_error = "No NATS server connection succeeded".to_owned();
            while let Some(result) = attempts.next().await {
                match result {
                    Ok(client) => return Ok(client),
                    Err(error) => last_error = error,
                }
            }
            Err(last_error)
        };
        // Dropping the losing futures releases tentative clients and connection attempts.
        match tokio::time::timeout(Duration::from_secs(4), race).await {
            Ok(result) => result,
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
    #[test]
    fn seed_lists_validate_shared_identity_and_stable_bindings() {
        let list = config(" nats://reader:secret@node-b:4223 , tls://node-a:4222 ");
        list.validate().unwrap();
        let reordered = config("tls://node-a:4222,nats://reader:rotated@node-b:4223");
        assert_eq!(list.binding("").unwrap(), reordered.binding("").unwrap());
        let repeated =
            config("tls://node-a:4222,nats://reader:secret@node-b:4223,nats://node-a:4222");
        assert_eq!(list.binding("").unwrap(), repeated.binding("").unwrap());
        assert_ne!(
            list.binding("").unwrap(),
            config("nats://reader:secret@node-b:4223")
                .binding("")
                .unwrap()
        );
        assert_ne!(list.binding("").unwrap(), list.binding("other").unwrap());
        assert_ne!(
            list.binding("").unwrap(),
            config("nats://other:secret@node-b:4223,nats://node-a:4222")
                .binding("")
                .unwrap()
        );
        assert_eq!(
            config("nats://reader:secret@localhost:4222")
                .binding("")
                .unwrap(),
            config("nats://localhost:4222,tls://reader:rotated@localhost:4222")
                .binding("")
                .unwrap()
        );
        for bad in [
            "",
            "nats://node-a,",
            ",nats://node-a",
            "nats://node-a,,nats://node-b",
            "nats://node-a,http://node-b",
            "nats://node-a,nats://node-b/path",
            "nats://reader:secret@node-a,nats://reader:different@node-b",
            "nats://token-one@node-a,nats://token-two@node-b",
        ] {
            let error = config(bad).validate().unwrap_err();
            assert!(
                !error.contains("secret")
                    && !error.contains("different")
                    && !error.contains("token-one")
            );
        }
        assert!(
            config(&vec!["nats://node"; 33].join(","))
                .validate()
                .is_err()
        );
        let mut conflicting = config("nats://node-a,nats://reader:secret@node-b");
        conflicting.credentials = Some("does-not-exist.creds".into());
        assert!(
            conflicting
                .validate()
                .unwrap_err()
                .contains("either NATSUI_CREDS")
        );
    }
}
