use crate::App;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::stream::{self, Stream};
use std::{
    convert::Infallible,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::Semaphore;

pub async fn updates(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    static LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = LIMIT
        .get_or_init(|| Arc::new(Semaphore::new(128)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let ticks = tokio::time::interval(Duration::from_secs(2));
    let stream = stream::unfold(
        (app, headers, ticks, None, permit),
        |(app, headers, mut ticks, last, permit)| async move {
            ticks.tick().await;
            app.auth.role(&headers)?;
            let profile = headers
                .get("x-natsui-profile")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("default");
            if !app.auth.allowed_profile(&headers, profile) {
                return None;
            }
            let at = app.current.read().await.observed_at;
            let event = if last != Some(at) {
                Event::default().event("changed").data(at.to_string())
            } else {
                Event::default().comment("alive")
            };
            Some((Ok(event), (app, headers, ticks, Some(at), permit)))
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
