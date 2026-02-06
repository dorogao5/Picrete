#[tokio::main]
async fn main() -> anyhow::Result<()> {
    picrete_rust::run_worker().await
}
