use crate::{
    create_api_error as api_error, validate_notification_request, ErrorCode, NotificationRequest,
    WindowsToastDispatcher, HEALTH_PATH, NOTIFICATIONS_PATH,
};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::CONTENT_TYPE, HeaderValue, Method, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use http_body_util::BodyExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, future::Future, io, net::SocketAddr, pin::Pin, sync::Arc};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use uuid::Uuid;

pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

pub type DispatchFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;
pub type LogContext = BTreeMap<String, Value>;

/// Structured logger used by the HTTP server.
pub trait NotifierLogger: Send + Sync + 'static {
    fn info(&self, message: &str, context: &LogContext);
    fn error(&self, message: &str, context: &LogContext);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ConsoleNotifierLogger;

impl NotifierLogger for ConsoleNotifierLogger {
    fn info(&self, message: &str, context: &LogContext) {
        println!("{message} {}", context_json(context));
    }

    fn error(&self, message: &str, context: &LogContext) {
        eprintln!("{message} {}", context_json(context));
    }
}

fn context_json(context: &LogContext) -> String {
    serde_json::to_string(context).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Clone)]
pub struct NotifierServerOptions {
    pub max_body_bytes: usize,
    dispatcher: Arc<dyn crate::NotificationDispatcher>,
    logger: Arc<dyn NotifierLogger>,
}

impl Default for NotifierServerOptions {
    fn default() -> Self {
        Self {
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            dispatcher: Arc::new(WindowsToastDispatcher),
            logger: Arc::new(ConsoleNotifierLogger),
        }
    }
}

impl NotifierServerOptions {
    pub fn with_dispatcher<D>(mut self, dispatcher: D) -> Self
    where
        D: crate::NotificationDispatcher,
    {
        self.dispatcher = Arc::new(dispatcher);
        self
    }

    /// Adapts a synchronous dispatcher to the server interface.
    pub fn with_sync_dispatcher<F>(mut self, dispatcher: F) -> Self
    where
        F: Fn(NotificationRequest, String) -> Result<(), String> + Send + Sync + 'static,
    {
        self.dispatcher = Arc::new(SyncDispatcher(dispatcher));
        self
    }

    pub fn with_logger<L>(mut self, logger: L) -> Self
    where
        L: NotifierLogger,
    {
        self.logger = Arc::new(logger);
        self
    }

    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }
}

struct SyncDispatcher<F>(F);

impl<F> crate::NotificationDispatcher for SyncDispatcher<F>
where
    F: Fn(NotificationRequest, String) -> Result<(), String> + Send + Sync + 'static,
{
    fn dispatch(&self, request: NotificationRequest, notification_id: String) -> DispatchFuture {
        let result = (self.0)(request, notification_id);
        Box::pin(async move { result })
    }
}

#[derive(Clone)]
struct ServerState {
    max_body_bytes: usize,
    dispatcher: Arc<dyn crate::NotificationDispatcher>,
    logger: Arc<dyn NotifierLogger>,
}

/// Creates an Axum router implementing the notifier HTTP protocol.
pub fn create_notifier_router(options: NotifierServerOptions) -> Router {
    let state = ServerState {
        max_body_bytes: options.max_body_bytes,
        dispatcher: options.dispatcher,
        logger: options.logger,
    };

    Router::new()
        .route(HEALTH_PATH, any(handle_health))
        .route(NOTIFICATIONS_PATH, any(handle_notifications))
        .fallback(handle_not_found)
        .with_state(state)
}

async fn handle_health(request: Request) -> Response {
    if request.method() != Method::GET {
        return method_not_allowed();
    }
    json_response(StatusCode::OK, json!({ "status": "ok" }))
}

async fn handle_notifications(State(state): State<ServerState>, request: Request) -> Response {
    if request.method() != Method::POST {
        return method_not_allowed();
    }

    let body = match read_body(request.into_body(), state.max_body_bytes).await {
        Ok(body) => body,
        Err(error) => {
            return json_response(
                error.status,
                api_error(ErrorCode::InvalidRequest, error.message),
            )
        }
    };

    let value = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                api_error(ErrorCode::InvalidRequest, "request body must be valid JSON"),
            )
        }
    };

    let notification = match validate_notification_request(value) {
        Ok(notification) => notification,
        Err(error) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                api_error(ErrorCode::InvalidRequest, error.message),
            )
        }
    };

    let notification_id = Uuid::new_v4().to_string();
    if let Err(error) = state
        .dispatcher
        .dispatch(notification.clone(), notification_id.clone())
        .await
    {
        let mut context =
            notification_log_context(&notification_id, notification.source.as_deref());
        context.insert("error".to_string(), Value::String(error));
        state
            .logger
            .error("Unable to submit notification", &context);
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(ErrorCode::InternalError, "unable to submit notification"),
        );
    }

    state.logger.info(
        "Notification submitted",
        &notification_log_context(&notification_id, notification.source.as_deref()),
    );
    json_response(
        StatusCode::CREATED,
        json!({ "notificationId": notification_id }),
    )
}

async fn handle_not_found(_request: Request) -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        api_error(ErrorCode::NotFound, "route not found"),
    )
}

fn notification_log_context(notification_id: &str, source: Option<&str>) -> LogContext {
    let mut context = LogContext::new();
    context.insert(
        "notificationId".to_string(),
        Value::String(notification_id.to_string()),
    );
    if let Some(source) = source {
        context.insert("source".to_string(), Value::String(source.to_string()));
    }
    context
}

fn method_not_allowed() -> Response {
    json_response(
        StatusCode::METHOD_NOT_ALLOWED,
        api_error(ErrorCode::MethodNotAllowed, "method not allowed"),
    )
}

fn json_response<T: Serialize>(status: StatusCode, payload: T) -> Response {
    let body = serde_json::to_vec(&payload).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

async fn read_body(mut body: Body, max_body_bytes: usize) -> Result<Vec<u8>, RequestBodyError> {
    let mut chunks = Vec::new();
    let mut byte_length = 0usize;

    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| RequestBodyError::bad_request())?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        byte_length = byte_length.saturating_add(data.len());
        if byte_length > max_body_bytes {
            return Err(RequestBodyError::too_large());
        }
        chunks.extend_from_slice(&data);
    }

    if chunks.is_empty() {
        return Err(RequestBodyError::bad_request());
    }
    Ok(chunks)
}

struct RequestBodyError {
    status: StatusCode,
    message: &'static str,
}

impl RequestBodyError {
    fn bad_request() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: "request body must be valid JSON",
        }
    }

    fn too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: "request body is too large",
        }
    }
}

#[derive(Clone)]
pub struct StartNotifierOptions {
    pub host: String,
    pub port: u16,
    pub server: NotifierServerOptions,
}

impl Default for StartNotifierOptions {
    fn default() -> Self {
        Self {
            host: crate::DEFAULT_NOTIFIER_HOST.to_string(),
            port: crate::DEFAULT_NOTIFIER_PORT,
            server: NotifierServerOptions::default(),
        }
    }
}

impl StartNotifierOptions {
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_server_options(mut self, server: NotifierServerOptions) -> Self {
        self.server = server;
        self
    }
}

/// A running notifier server. Dropping it closes the shutdown signal; call
/// [`RunningNotifier::shutdown`] to stop the listener and await graceful completion.
pub struct RunningNotifier {
    local_addr: SocketAddr,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<io::Result<()>>,
}

impl RunningNotifier {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn shutdown(self) -> io::Result<()> {
        let Self {
            local_addr: _,
            shutdown,
            task,
        } = self;
        let _ = shutdown.send(());
        task.await.map_err(io::Error::other)?
    }
}

/// Binds a TCP listener, starts the HTTP service, and returns once it is listening.
pub async fn start_notifier(options: StartNotifierOptions) -> io::Result<RunningNotifier> {
    let listener = TcpListener::bind((options.host.as_str(), options.port)).await?;
    let local_addr = listener.local_addr()?;
    let router = create_notifier_router(options.server);
    let (shutdown, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    Ok(RunningNotifier {
        local_addr,
        shutdown,
        task,
    })
}
