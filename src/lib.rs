//! Rust implementation of `@attentive-kit/notifier`.
//!
//! The crate exposes the notifier HTTP protocol, a configurable Axum router, and a
//! Windows toast dispatcher. On non-Windows systems, applications can provide their
//! own [`NotificationDispatcher`] for development or alternative notification backends.

pub mod protocol;
pub mod server;
pub mod toast;

pub use protocol::{
    create_api_error, is_http_url, is_json_object, is_open_uri, validate_notification_request,
    ApiErrorResponse, CreateNotificationResponse, ErrorCode, InvalidNotificationRequestError,
    JsonObject, NotificationRequest, OpenUriAction, API_VERSION, DEFAULT_NOTIFIER_HOST,
    DEFAULT_NOTIFIER_PORT, DEFAULT_NOTIFIER_URL, HEALTH_PATH, MAX_OPEN_URI_LENGTH,
    NOTIFICATIONS_PATH, OPEN_URI_SCHEMES,
};
pub use server::{
    create_notifier_router, start_notifier, DispatchFuture, LogContext, NotifierLogger,
    NotifierServerOptions, RunningNotifier, StartNotifierOptions, DEFAULT_MAX_BODY_BYTES,
};
pub use toast::{NotificationDispatcher, WindowsToastDispatcher};
