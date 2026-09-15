use crate::{App, auth};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;
type Error = (StatusCode, &'static str);
static RUNTIME: OnceLock<Option<Arc<Runtime>>> = OnceLock::new();
static FLOW_LIMIT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);
const FLOW_COOKIE: &str = "__Host-natsui_oidc";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    issuer: String,
    client_id: String,
    client_secret_file: Option<String>,
    ca_file: Option<String>,
    subjects: HashMap<String, Mapping>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    user: String,
    identity_revision: u64,
}
struct Runtime {
    config: Configuration,
    secret: Option<ClientSecret>,
    redirect: RedirectUrl,
    http: reqwest::Client,
    pending: Mutex<HashMap<String, Pending>>,
}
struct Pending {
    cookie: [u8; 32],
    nonce: Nonce,
    verifier: PkceCodeVerifier,
    expires: Instant,
}
fn failure() -> Error {
    (
        StatusCode::UNAUTHORIZED,
        "Single sign-on failed. Retry sign-in or use the access-key recovery path.",
    )
}
fn private_file(path: &str, limit: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err("Empty or oversized OIDC configuration file".into());
    }
    Ok(bytes)
}
fn https(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}
pub fn initialize(auth: &auth::Auth) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = if let Ok(path) = std::env::var("NATSUI_OIDC_CONFIG_FILE") {
        let origin = auth
            .origin()
            .ok_or("OIDC requires a configured HTTPS public origin and recovery key")?;
        let config: Configuration = serde_json::from_slice(&private_file(&path, 65536)?)?;
        let issuer = reqwest::Url::parse(&config.issuer)?;
        if !https(&issuer)
            || issuer.query().is_some()
            || config.client_id.is_empty()
            || config.client_id.len() > 256
            || config.subjects.is_empty()
            || config.subjects.len() > 256
            || config.subjects.iter().any(|(subject, mapping)| {
                let user = &mapping.user;
                mapping.identity_revision == 0
                    || subject.is_empty()
                    || subject.len() > 255
                    || user.is_empty()
                    || user.len() > 48
                    || user == "bootstrap"
                    || !user.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'
                    })
            })
        {
            return Err("Invalid OIDC issuer, client or subject mapping".into());
        }
        let secret = config
            .client_secret_file
            .as_ref()
            .map(|path| -> Result<_, Box<dyn std::error::Error>> {
                let secret = String::from_utf8(private_file(path, 8192)?)?
                    .trim_end_matches(['\r', '\n'])
                    .to_owned();
                if secret.is_empty() {
                    return Err("Empty OIDC client secret".into());
                }
                Ok(ClientSecret::new(secret))
            })
            .transpose()?;
        let mut http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        if let Some(path) = &config.ca_file {
            http = http
                .add_root_certificate(reqwest::Certificate::from_pem(&private_file(path, 65536)?)?);
        }
        Some(Arc::new(Runtime {
            redirect: RedirectUrl::new(origin.join("api/auth/oidc/callback")?.to_string())?,
            config,
            secret,
            http: http.build()?,
            pending: Mutex::new(HashMap::new()),
        }))
    } else {
        None
    };
    RUNTIME
        .set(runtime)
        .map_err(|_| "OIDC was initialized twice")?;
    Ok(())
}
fn runtime() -> Result<Arc<Runtime>, Error> {
    RUNTIME
        .get()
        .and_then(Clone::clone)
        .ok_or((StatusCode::NOT_FOUND, "Single sign-on is not configured"))
}
impl Runtime {
    // The same bounded HTTPS client handles discovery, JWKS and token exchange.
    async fn request(
        &self,
        request: openidconnect::HttpRequest,
    ) -> Result<openidconnect::HttpResponse, std::io::Error> {
        let (parts, body) = request.into_parts();
        let url = reqwest::Url::parse(&parts.uri.to_string()).map_err(std::io::Error::other)?;
        if !https(&url) {
            return Err(std::io::Error::other("OIDC endpoints must use HTTPS"));
        }
        let mut response = self
            .http
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(std::io::Error::other)?;
        let status = response.status();
        let headers = response.headers().clone();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(std::io::Error::other)? {
            if bytes.len() + chunk.len() > 1048576 {
                return Err(std::io::Error::other("OIDC response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let mut response = openidconnect::HttpResponse::new(bytes);
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        Ok(response)
    }
    async fn client(&self) -> Result<Client, Error> {
        let metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new(self.config.issuer.clone()).map_err(|_| failure())?,
            self,
        )
        .await
        .map_err(|_| failure())?;
        if !https(metadata.authorization_endpoint().url()) {
            return Err(failure());
        }
        Ok(CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.config.client_id.clone()),
            self.secret.clone(),
        )
        .set_redirect_uri(self.redirect.clone()))
    }
}
pub async fn available() -> Json<serde_json::Value> {
    Json(json!({"enabled":runtime().is_ok()}))
}
pub async fn start() -> Result<Response, Error> {
    let runtime = runtime()?;
    let _permit = FLOW_LIMIT.try_acquire().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many concurrent sign-ins",
        )
    })?;
    let client = runtime.client().await?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .set_pkce_challenge(challenge)
        .url();
    let cookie = CsrfToken::new_random().secret().to_owned();
    let mut pending = runtime.pending.lock().map_err(|_| failure())?;
    pending.retain(|_, flow| flow.expires > Instant::now());
    if pending.len() >= 32 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Too many sign-in attempts. Retry shortly.",
        ));
    }
    pending.insert(
        state.secret().to_owned(),
        Pending {
            cookie: Sha256::digest(cookie.as_bytes()).into(),
            nonce,
            verifier,
            expires: Instant::now() + Duration::from_secs(300),
        },
    );
    let mut response = Redirect::temporary(url.as_str()).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!("{FLOW_COOKIE}={cookie}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=300")
            .parse()
            .map_err(|_| failure())?,
    );
    Ok(response)
}
#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: String,
}
pub async fn callback(
    State(app): State<App>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Result<Response, Error> {
    let runtime = runtime()?;
    let _permit = FLOW_LIMIT.try_acquire().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many concurrent sign-ins",
        )
    })?;
    if query.state.len() > 256 || query.code.as_ref().is_none_or(|code| code.len() > 8192) {
        return Err(failure());
    }
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .filter_map(|v| v.trim().split_once('='))
                .find_map(|(key, value)| (key == FLOW_COOKIE).then_some(value))
        })
        .ok_or_else(failure)?;
    let flow = runtime
        .pending
        .lock()
        .map_err(|_| failure())?
        .remove(&query.state)
        .ok_or_else(failure)?;
    let hashed: [u8; 32] = Sha256::digest(cookie.as_bytes()).into();
    if flow.expires <= Instant::now() || !bool::from(hashed.ct_eq(&flow.cookie)) {
        return Err(failure());
    }
    let client = runtime.client().await?;
    let tokens = client
        .exchange_code(AuthorizationCode::new(query.code.ok_or_else(failure)?))
        .map_err(|_| failure())?
        .set_pkce_verifier(flow.verifier)
        .request_async(runtime.as_ref())
        .await
        .map_err(|_| failure())?;
    let token = tokens.id_token().ok_or_else(failure)?;
    let verifier = client.id_token_verifier();
    let claims = token
        .claims(&verifier, &flow.nonce)
        .map_err(|_| failure())?;
    if let Some(expected) = claims.access_token_hash() {
        let actual = AccessTokenHash::from_token(
            tokens.access_token(),
            token.signing_alg().map_err(|_| failure())?,
            token.signing_key(&verifier).map_err(|_| failure())?,
        )
        .map_err(|_| failure())?;
        if &actual != expected {
            return Err(failure());
        }
    }
    if claims
        .authorized_party()
        .is_some_and(|party| party.as_str() != runtime.config.client_id)
        || (claims.audiences().len() > 1 && claims.authorized_party().is_none())
    {
        return Err(failure());
    }
    let user = runtime
        .config
        .subjects
        .get(claims.subject().as_str())
        .ok_or_else(failure)?;
    let session = app
        .auth
        .external_session(&user.user, user.identity_revision, &headers)
        .map_err(|_| failure())?;
    let mut response = axum::response::Html("<!doctype html><html><head><meta charset=\"utf-8\"><title>Signed in - natsui</title><script src=\"/oidc-complete.js\" defer></script></head><body><p>Signed in. Opening the dashboard...</p><a href=\"/\">Continue</a></body></html>").into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, session.parse().map_err(|_| failure())?);
    response.headers_mut().append(
        header::SET_COOKIE,
        format!("{FLOW_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0")
            .parse()
            .map_err(|_| failure())?,
    );
    Ok(response)
}

impl<'a> openidconnect::AsyncHttpClient<'a> for Runtime {
    type Error = std::io::Error;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<openidconnect::HttpResponse, Self::Error>>
                + Send
                + 'a,
        >,
    >;
    fn call(&'a self, request: openidconnect::HttpRequest) -> Self::Future {
        Box::pin(self.request(request))
    }
}
