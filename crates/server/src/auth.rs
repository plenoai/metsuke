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
pub struct OAuthAuthLayer {
    db: Arc<Database>,
    resource_metadata_url: String,
}

impl OAuthAuthLayer {
    pub fn new(db: Arc<Database>, base_url: &str) -> Self {
        Self {
            db,
            resource_metadata_url: format!("{base_url}/.well-known/oauth-protected-resource"),
        }
    }
}

impl<S> Layer<S> for OAuthAuthLayer {
    type Service = OAuthAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        OAuthAuthService {
            inner,
            db: self.db.clone(),
            resource_metadata_url: self.resource_metadata_url.clone(),
        }
    }
}

#[derive(Clone)]
pub struct OAuthAuthService<S> {
    inner: S,
    db: Arc<Database>,
    resource_metadata_url: String,
}

fn unauthorized_response(resource_metadata_url: &str) -> Response<BoxBody<Bytes, Infallible>> {
    let body = Full::new(Bytes::from(
        r#"{"error":"unauthorized","message":"OAuth 2.1 authorization required. See /.well-known/oauth-protected-resource for metadata."}"#,
    ))
    .map_err(|never: Infallible| match never {})
    .boxed();

    // RFC 9728: include resource_metadata in WWW-Authenticate
    let www_authenticate = format!("Bearer resource_metadata=\"{resource_metadata_url}\"");

    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("WWW-Authenticate", www_authenticate)
        .header("Content-Type", "application/json")
        .body(body)
        .unwrap()
}

impl<S> Service<Request<Body>> for OAuthAuthService<S>
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
        let resource_metadata_url = self.resource_metadata_url.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let access_token = match token {
                Some(t) => t,
                None => return Ok(unauthorized_response(&resource_metadata_url)),
            };

            // Validate OAuth access token
            let user_id = match db.validate_access_token(&access_token) {
                Ok(Some(id)) => id,
                _ => return Ok(unauthorized_response(&resource_metadata_url)),
            };

            REQUEST_USER_ID.scope(user_id, inner.call(req)).await
        })
    }
}
