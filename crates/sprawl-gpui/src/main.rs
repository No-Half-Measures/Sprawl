mod app;
mod format;
mod layout;
mod state;
#[cfg(test)]
mod tests;
mod treemap;

use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};
use std::{path::PathBuf, time::Instant};

fn main() {
    let startup = Instant::now();
    let mut data = dirs::data_local_dir()
        .expect("local data directory")
        .join(sprawl_core::cli::IDENTIFIER);
    let mut open = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data = PathBuf::from(args.next().expect("--data-dir requires a path")),
            "--open-saved" => open = Some(args.next().expect("--open-saved requires a path")),
            _ => {
                eprintln!("Unknown argument: {arg}");
                std::process::exit(2);
            }
        }
    }
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(640.), px(400.))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Sprawl · GPUI".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| cx.new(|cx| app::DiskApp::new(data, open, startup, cx)),
        )
        .expect("open GPUI window");
        cx.activate(true);
    });
}
