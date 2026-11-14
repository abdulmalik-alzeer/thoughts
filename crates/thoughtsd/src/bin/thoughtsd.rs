fn main() -> anyhow::Result<()> {
    if std::env::var("THOUGHTS_ASKPASS").is_ok() {
        std::process::exit(thoughtsd::askpass::main());
    }
    if std::env::args().nth(1).as_deref() == Some(thoughtsd::shell_guard::FLAG) {
        std::process::exit(thoughtsd::shell_guard::main());
    }
    tokio::runtime::Runtime::new()?.block_on(thoughtsd::run())
}
