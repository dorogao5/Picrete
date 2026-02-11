#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(e) = picrete_rust::run_telegram_bot().await {
        eprintln!("picrete-telegram-bot fatal: {e:#}");
        std::process::exit(1);
    }
    Ok(())
}
