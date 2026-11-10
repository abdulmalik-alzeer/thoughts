fn main() -> anyhow::Result<()> {
    if std::env::var("THOUGHTS_ASKPASS").is_ok() {
        std::process::exit(thoughtsd::askpass::main());
    }
    tokio::runtime::Runtime::new()?.block_on(thoughtsd::run())
}
