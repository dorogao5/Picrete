#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(e) = picrete_rust::run().await {
        eprintln!("picrete-rust fatal: {e:#}");
        std::process::exit(1);
    }
    Ok(())
}
