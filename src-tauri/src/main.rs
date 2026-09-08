// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    configure_linux_display_backend();
    searchmonkey_lib::run()
}

#[cfg(target_os = "linux")]
fn configure_linux_display_backend() {
    // GTK's native Wayland decorations do not currently honour KDE's configured
    // titlebar-button layout. Prefer XWayland on KDE until that integration is
    // reliable, while preserving an explicit user choice.
    if std::env::var_os("GDK_BACKEND").is_none()
        && std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
        && std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|desktop| desktop.to_ascii_lowercase().contains("kde"))
    {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_display_backend() {}
