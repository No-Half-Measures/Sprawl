mod actions;
mod app;
mod format;
mod input;
mod layout;
mod preferences;
mod single_instance;
mod state;
#[cfg(test)]
mod tests;
mod treemap;

use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};
use std::time::Instant;

fn main() {
    let startup = Instant::now();
    #[cfg(windows)]
    if let Err(error) = sprawl_core::admin::wait_for_parent() {
        eprintln!("{error}");
        return;
    }
    let args: Vec<_> = std::env::args().collect();
    let parsed = match actions::arguments(args.iter().skip(1).cloned()) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let mut instance = match single_instance::start(&args) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };
    let receiver = instance.receiver.take();
    let data = parsed.data.unwrap_or_else(|| {
        dirs::data_local_dir()
            .expect("local data directory")
            .join(sprawl_core::cli::IDENTIFIER)
    });
    let launch = sprawl_core::shell::launch_path(parsed.remaining);
    let open = parsed.open;
    Application::new().run(move |cx: &mut App| {
        input::bind(cx);
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(760.), px(600.))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Sprawl · GPUI".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let handle = window.window_handle();
                cx.new(|cx| app::DiskApp::new(data, open, launch, receiver, startup, handle, cx))
            },
        )
        .expect("open GPUI window");
        cx.activate(true);
    });
}
