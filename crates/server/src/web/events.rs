use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::response::sse::{Event, KeepAlive, Sse};

use super::JobEvent;
use super::WebState;
use super::helpers::require_user;

pub(super) async fn api_events(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let mut rx = state.events_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let event_user_id = match &event {
                        JobEvent::ReposSynced { user_id, .. } => *user_id,
                        JobEvent::PullsSynced { user_id, .. } => *user_id,
                        JobEvent::ReleasesSynced { user_id, .. } => *user_id,
                        JobEvent::VerificationComplete { user_id, .. } => *user_id,
                    };
                    if event_user_id == user_id {
                        let data = serde_json::to_string(&event).unwrap_or_default();
                        yield Ok::<_, std::convert::Infallible>(
                            Event::default().event("job").data(data)
                        );
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
