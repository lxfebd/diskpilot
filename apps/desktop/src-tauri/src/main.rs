#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // R513: --self-test 无头自检模式（CI 跨平台冒烟用），不建窗口直接跑完退出。
    if std::env::args().any(|a| a == "--self-test") {
        std::process::exit(diskpilot_desktop_lib::self_test::run());
    }
    diskpilot_desktop_lib::run();
}
