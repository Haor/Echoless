// Echoless desktop GUI entrypoint. Hide the GUI console on Windows for both
// debug smoke bundles and release installers; the separate CLI stays console-capable.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    echoless_app_lib::run()
}
