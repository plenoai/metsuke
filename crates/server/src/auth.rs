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

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use tower::{ServiceBuilder, ServiceExt};

    fn test_db() -> Arc<Database> {
        let path = std::env::temp_dir()
            .join(format!("metsuke-test-{}.db", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        Arc::new(Database::open(&path).unwrap())
    }

    #[test]
    fn unauthorized_response_has_correct_status_and_headers() {
        let resp =
            unauthorized_response("https://example.com/.well-known/oauth-protected-resource");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www_auth = resp
            .headers()
            .get("WWW-Authenticate")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(www_auth.contains("Bearer"));
        assert!(www_auth.contains("resource_metadata="));
        let ct = resp
            .headers()
            .get("Content-Type")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(ct, "application/json");
    }

    fn echo_body(s: &str) -> BoxBody<Bytes, Infallible> {
        Full::new(Bytes::from(s.to_string()))
            .map_err(|never: Infallible| match never {})
            .boxed()
    }

    /// Build a minimal echo service that returns 200 with the user_id from task-local
    fn echo_service() -> impl Service<
        Request<Body>,
        Response = Response<BoxBody<Bytes, Infallible>>,
        Error = Infallible,
        Future = Pin<
            Box<
                dyn Future<Output = Result<Response<BoxBody<Bytes, Infallible>>, Infallible>>
                    + Send,
            >,
        >,
    > + Clone {
        tower::service_fn(|_req: Request<Body>| {
            Box::pin(async move {
                let uid = REQUEST_USER_ID.try_with(|id| *id).unwrap_or(-1);
                let body = echo_body(&uid.to_string());
                Ok::<_, Infallible>(
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(body)
                        .unwrap(),
                )
            })
                as Pin<
                    Box<
                        dyn Future<
                                Output = Result<Response<BoxBody<Bytes, Infallible>>, Infallible>,
                            > + Send,
                    >,
                >
        })
    }

    #[tokio::test]
    async fn rejects_request_without_auth_header() {
        let db = test_db();
        let svc = ServiceBuilder::new()
            .layer(OAuthAuthLayer::new(db, "https://example.com"))
            .service(echo_service());

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_invalid_token() {
        let db = test_db();
        let svc = ServiceBuilder::new()
            .layer(OAuthAuthLayer::new(db, "https://example.com"))
            .service(echo_service());

        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Bearer invalid-token")
            .body(Body::empty())
            .unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_non_bearer_auth() {
        let db = test_db();
        let svc = ServiceBuilder::new()
            .layer(OAuthAuthLayer::new(db, "https://example.com"))
            .service(echo_service());

        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn accepts_valid_token_and_sets_user_id() {
        let db = test_db();
        // Create user and token
        let uid = db.upsert_user(1, "testuser", None, None).unwrap();
        db.register_oauth_client("cid", None, None, &["https://cb".into()], "none")
            .unwrap();
        db.create_oauth_token("valid-access-token", "rt", "cid", uid, "mcp", 3600, 86400)
            .unwrap();

        let svc = ServiceBuilder::new()
            .layer(OAuthAuthLayer::new(db, "https://example.com"))
            .service(echo_service());

        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Bearer valid-access-token")
            .body(Body::empty())
            .unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Body should contain the user_id
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(body_str, uid.to_string());
    }
}
