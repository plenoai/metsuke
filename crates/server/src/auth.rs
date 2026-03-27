use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use tower::{Layer, Service};

use crate::db::Database;

tokio::task_local! {
    pub static REQUEST_USER_ID: i64;
}

#[derive(Clone)]
pub struct BearerAuthLayer {
    db: Arc<Database>,
}

impl BearerAuthLayer {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl<S> Layer<S> for BearerAuthLayer {
    type Service = BearerAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        BearerAuthService {
            inner,
            db: self.db.clone(),
        }
    }
}

#[derive(Clone)]
pub struct BearerAuthService<S> {
    inner: S,
    db: Arc<Database>,
}

fn unauthorized_response() -> Response<BoxBody<Bytes, Infallible>> {
    let body = Full::new(Bytes::from(
        r#"{"error":"unauthorized","message":"Authorization: Bearer <session-token> required"}"#,
    ))
    .map_err(|never: Infallible| match never {})
    .boxed();
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("WWW-Authenticate", "Bearer")
        .header("Content-Type", "application/json")
        .body(body)
        .unwrap()
}

impl<S> Service<Request<Body>> for BearerAuthService<S>
where
    S: Service<Request<Body>, Response = Response<BoxBody<Bytes, Infallible>>>
        + Clone
        + Send
        + 'static,
    S::Error: Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<BoxBody<Bytes, Infallible>>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let token = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|t| t.to_string());

        let db = self.db.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let session_id = match token {
                Some(t) => t,
                None => return Ok(unauthorized_response()),
            };

            let user_id = match db.get_user_by_session(&session_id) {
                Ok(Some((id, _))) => id,
                _ => return Ok(unauthorized_response()),
            };

            REQUEST_USER_ID.scope(user_id, inner.call(req)).await
        })
    }
}
