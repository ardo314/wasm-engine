use std::time::Duration;

use clap::Parser;
use wasm_registry::{DEFAULT_BUCKET, Limits, Registry, serve};

/// Serves the ardo314:registry interface over NATS.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// NATS server to connect to.
    #[arg(long, env = "NATS_URL", default_value = "nats://127.0.0.1:4222")]
    nats: String,

    /// JetStream key-value bucket holding provider registrations.
    #[arg(long, env = "REGISTRY_BUCKET", default_value = DEFAULT_BUCKET)]
    bucket: String,

    /// Shortest TTL a provider may propose, in seconds.
    #[arg(long, default_value_t = 5)]
    min_ttl_secs: u64,

    /// Longest TTL a provider may propose, in seconds.
    #[arg(long, default_value_t = 3600)]
    max_ttl_secs: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    anyhow::ensure!(
        args.min_ttl_secs >= 1 && args.min_ttl_secs <= args.max_ttl_secs,
        "--min-ttl-secs must be between 1 and --max-ttl-secs"
    );

    let client = async_nats::connect(&args.nats).await?;
    let registry = Registry::open(
        &async_nats::jetstream::new(client.clone()),
        &args.bucket,
        Limits {
            min_ttl: Duration::from_secs(args.min_ttl_secs),
            max_ttl: Duration::from_secs(args.max_ttl_secs),
        },
    )
    .await?;

    println!(
        "registryd serving bucket `{}` on {}",
        args.bucket, args.nats
    );
    serve(client, registry).await
}
