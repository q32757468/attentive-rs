use crate::protocol::NotificationRequest;
use crate::server::DispatchFuture;

/// Submits a validated notification to an operating-system notification backend.
pub trait NotificationDispatcher: Send + Sync + 'static {
    fn dispatch(&self, request: NotificationRequest, notification_id: String) -> DispatchFuture;
}

/// Default dispatcher that submits notifications to Windows Toast.
///
/// On non-Windows hosts, dispatching returns an error. Supply another dispatcher through
/// [`crate::NotifierServerOptions`] when using the server on another platform.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsToastDispatcher;

impl NotificationDispatcher for WindowsToastDispatcher {
    fn dispatch(&self, request: NotificationRequest, notification_id: String) -> DispatchFuture {
        #[cfg(windows)]
        {
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    let _notification_id = notification_id;
                    show_windows_toast(request)
                })
                .await
                .map_err(|error| format!("Windows notification worker failed: {error}"))?
            })
        }

        #[cfg(not(windows))]
        {
            let _ = (request, notification_id);
            Box::pin(async {
                Err("Windows notifications require a Windows notifier host".to_string())
            })
        }
    }
}

#[cfg(windows)]
const APP_USER_MODEL_ID: &str = "Attentive.Notifier";
#[cfg(windows)]
const APP_DISPLAY_NAME: &str = "Attentive Notifier";

#[cfg(windows)]
fn show_windows_toast(request: NotificationRequest) -> Result<(), String> {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Once,
    };
    use winrt_toast_reborn::{Toast, ToastManager};

    static REGISTER_TOAST_APP: Once = Once::new();
    REGISTER_TOAST_APP.call_once(|| {
        let _ = winrt_toast_reborn::register(APP_USER_MODEL_ID, APP_DISPLAY_NAME, None);
    });

    let mut toast = Toast::new();
    toast.text1(request.title).text2(request.body);

    let opened = Arc::new(AtomicBool::new(false));
    let action_uri = request.action.map(|action| action.uri);
    if let Some(uri) = action_uri.as_deref() {
        toast.launch(uri);
    }

    let opened_on_activation = Arc::clone(&opened);
    let manager = ToastManager::new(APP_USER_MODEL_ID).on_activated(None, move |_action| {
        if let Some(uri) = action_uri.as_deref() {
            if !opened_on_activation.swap(true, Ordering::SeqCst) {
                let _ = open_uri_with_explorer(uri);
            }
        }
    });

    manager
        .show(&toast)
        .map_err(|error| format!("Windows toast submission failed: {error}"))
}

#[cfg(windows)]
fn open_uri_with_explorer(uri: &str) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(uri)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("unable to open notification action: {error}"))
}
