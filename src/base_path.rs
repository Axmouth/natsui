// Serves the dashboard under the path of NATSUI_PUBLIC_URL, so a reverse proxy can forward
// requests unchanged, without rewriting paths or response bodies. Routes stay root-relative
// inside. Requests without the prefix are still served, for container health checks and the
// local login CLI.
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Uri, header, uri::PathAndQuery};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use std::sync::Arc;

pub fn from_origin(origin: Option<&reqwest::Url>) -> String {
    origin
        .map(|url| url.path().trim_end_matches('/').to_owned())
        .unwrap_or_default()
}

pub async fn serve(State(base): State<Arc<str>>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if path == &*base {
        let query = request
            .uri()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
        return Redirect::permanent(&format!("{base}/{query}")).into_response();
    }
    if let Some(inner) = path.strip_prefix(&*base).filter(|p| p.starts_with('/')) {
        let inner = match request.uri().query() {
            Some(query) => format!("{inner}?{query}"),
            None => inner.to_owned(),
        };
        let mut parts = request.uri().clone().into_parts();
        parts.path_and_query = PathAndQuery::try_from(inner).ok();
        match Uri::from_parts(parts) {
            Ok(uri) => *request.uri_mut() = uri,
            Err(_) => return axum::http::StatusCode::BAD_REQUEST.into_response(),
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    if let Some(location) = headers.get(header::LOCATION).and_then(|v| v.to_str().ok())
        && location.starts_with('/')
        && !location.starts_with("//")
        && let Ok(value) = HeaderValue::from_str(&format!("{base}{location}"))
    {
        headers.insert(header::LOCATION, value);
    }
    let cookies: Vec<_> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|cookie| {
            cookie
                .split("; ")
                .map(|part| {
                    if part == "Path=/" {
                        format!("Path={base}/")
                    } else {
                        part.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .collect();
    if !cookies.is_empty() {
        headers.remove(header::SET_COOKIE);
        for cookie in cookies {
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                headers.append(header::SET_COOKIE, value);
            }
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::StatusCode, routing::get};
    use tower::{Layer, ServiceExt};

    fn app() -> impl tower::Service<
        Request,
        Response = Response,
        Error = std::convert::Infallible,
        Future = impl Send,
    > + Clone {
        let router = Router::new()
            .route("/", get(|| async { "index" }))
            .route(
                "/api/records/{stream}",
                get(|stream: axum::extract::Path<String>| async move { stream.0 }),
            )
            .route(
                "/private",
                get(|| async {
                    (
                        [(header::SET_COOKIE, "session=x; Path=/; HttpOnly")],
                        Redirect::to("/login"),
                    )
                }),
            )
            .route(
                "/away",
                get(|| async { Redirect::to("https://idp.example/auth") }),
            );
        axum::middleware::from_fn_with_state(Arc::<str>::from("/tools/natsui"), serve).layer(router)
    }

    async fn get_path(path: &str) -> Response {
        app()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[test]
    fn base_comes_from_the_public_url_path() {
        let url = |s: &str| reqwest::Url::parse(s).unwrap();
        assert_eq!(from_origin(None), "");
        assert_eq!(from_origin(Some(&url("https://dash.example/"))), "");
        assert_eq!(
            from_origin(Some(&url("https://dash.example/tools/natsui/"))),
            "/tools/natsui"
        );
    }

    #[tokio::test]
    async fn serves_under_the_base_and_at_the_root() {
        for path in ["/tools/natsui/", "/"] {
            let response = get_path(path).await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
        let response = get_path("/tools/natsui?x=1").await;
        assert_eq!(response.headers()[header::LOCATION], "/tools/natsui/?x=1");
        assert_eq!(
            get_path("/tools/natsuix/").await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn keeps_encoded_slashes_in_one_segment() {
        let response = get_path("/tools/natsui/api/records/databus%2FCASES").await;
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"databus/CASES");
    }

    #[tokio::test]
    async fn prefixes_redirects_and_cookie_paths() {
        let response = get_path("/tools/natsui/private").await;
        assert_eq!(response.headers()[header::LOCATION], "/tools/natsui/login");
        assert_eq!(
            response.headers()[header::SET_COOKIE],
            "session=x; Path=/tools/natsui/; HttpOnly"
        );
        let response = get_path("/tools/natsui/away").await;
        assert_eq!(
            response.headers()[header::LOCATION],
            "https://idp.example/auth"
        );
    }
}
