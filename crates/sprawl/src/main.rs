#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
mod theme;
mod treemap;

use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};
use std::time::Instant;

fn main() {
    let startup = Instant::now();
    #[cfg(windows)]
    if let Err(error) = sprawl_core::admin::wait_for_parent() {
        startup_error(&error);
        std::process::exit(1);
    }
    let args: Vec<_> = std::env::args().collect();
    let parsed = match actions::arguments(args.iter().skip(1).cloned()) {
        Ok(args) => args,
        Err(error) => {
            startup_error(&error);
            std::process::exit(2);
        }
    };
    let mut instance = match single_instance::start(&args) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            startup_error(&error);
            std::process::exit(1);
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
    let appearance = parsed.appearance;
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        #[cfg(target_os = "macos")]
        macos_menu(cx);
        input::bind(cx);
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(760.), px(600.))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Sprawl".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                cx.set_global(theme::Palette::from_appearance(
                    appearance.unwrap_or_else(|| window.appearance()),
                ));
                let handle = window.window_handle();
                cx.new(|cx| {
                    cx.observe_window_appearance(
                        window,
                        move |this: &mut app::DiskApp, window, cx| {
                            this.set_palette(
                                theme::Palette::from_appearance(
                                    appearance.unwrap_or_else(|| window.appearance()),
                                ),
                                cx,
                            );
                        },
                    )
                    .detach();
                    app::DiskApp::new(data, open, launch, receiver, startup, handle, cx)
                })
            },
        )
        .expect("open GPUI window");
        cx.activate(true);
    });
}

fn startup_error(error: &str) {
    eprintln!("{error}");
    #[cfg(all(windows, not(debug_assertions)))]
    {
        let message: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "Sprawl".encode_utf16().chain(Some(0)).collect();
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                    | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
            );
        }
    }
}

#[cfg(windows)]
const FONT_FAMILY: &str = "Segoe UI";
#[cfg(target_os = "macos")]
const FONT_FAMILY: &str = ".SystemUIFont";
#[cfg(not(any(windows, target_os = "macos")))]
const FONT_FAMILY: &str = "DejaVu Sans";

#[cfg(target_os = "macos")]
gpui::actions!(sprawl, [Quit]);

#[cfg(target_os = "macos")]
fn macos_menu(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([gpui::KeyBinding::new("cmd-q", Quit, None)]);
    cx.set_menus(vec![gpui::Menu {
        name: "Sprawl".into(),
        items: vec![gpui::MenuItem::action("Quit Sprawl", Quit)],
    }]);
}
