#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(e) = picrete_rust::run_worker().await {
        eprintln!("picrete-worker fatal: {e:#}");
        std::process::exit(1);
    }
    Ok(())
}
