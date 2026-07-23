fn main() {
    if std::env::args().any(|argument| argument == "--server") {
        tranova_lib::run_server();
    } else {
        tranova_lib::run();
    }
}
