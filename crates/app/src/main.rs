mod actions;
mod notification;
mod state;
mod views;

use std::sync::{Arc, Mutex};

use gpui::{px, size, AppContext as _, Application, Bounds, WindowBounds, WindowOptions};
use rqs_lib::{Visibility, RQS};
use rqs_settings::Settings;

use crate::state::{spawn_channel_listener, spawn_discovery_listener, AppState};
use crate::views::root::RootView;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,blade_graphics=warn,naga=warn"),
    )
    .init();

    let settings = Settings::load();
    let hostname = gethostname::gethostname().to_string_lossy().into_owned();

    let visibility = Visibility::from_raw_value(settings.visibility as u64);
    let port = settings.port;
    let download_path = settings.download_path.clone();

    // Build tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");

    // Initialize core
    let mut rqs = RQS::new(visibility, port, download_path);
    let message_sender = rqs.message_sender.clone();
    let message_rx = message_sender.subscribe();

    let (sender_file, _ble_rx) = rt
        .block_on(async { rqs.run().await })
        .expect("Failed to start RQS core");

    let rqs = Arc::new(Mutex::new(rqs));

    // Enter tokio context for the rest of the app lifetime
    let guard = rt.enter();

    Application::new().run(|cx| {
        gpui_component::init(cx);

        let state = cx.new(|_cx| {
            let mut app_state = AppState::new(settings, hostname);
            app_state.sender_file = Some(sender_file);
            app_state.message_sender = Some(message_sender);
            app_state.rqs = Some(rqs);
            app_state
        });

        // Spawn background listeners on app-level executor
        let dch_rx = state.read(cx).dch_sender.subscribe();
        spawn_channel_listener(state.clone(), message_rx, cx);
        spawn_discovery_listener(state.clone(), dch_rx, cx);

        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("RQuickShare".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let root_view: gpui::Entity<RootView> =
                    cx.new(|cx| RootView::new(state, window, cx));
                let any_view: gpui::AnyView = root_view.into();
                cx.new(|cx| gpui_component::Root::new(any_view, window, cx))
            },
        )
        .expect("Failed to open window");
    });

    // Explicitly drop the enter guard before the runtime to avoid
    // "EnterGuard values dropped out of order" panic on shutdown
    drop(guard);
}
