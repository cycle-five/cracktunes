use crack_testing::run;
use tracing::{instrument, warn};

#[cfg(feature = "crack-tracing")]
#[instrument]
/// Initialize logging and tracing.
fn init_logging() {
    tracing_subscriber::fmt().init();
    warn!("Hello, world!");
}

/// Main function
#[tokio::main]
#[cfg(not(tarpaulin_include))]
async fn main() {
    #[cfg(feature = "crack-tracing")]
    init_logging();
    #[cfg(not(feature = "crack-tracing"))]
    println!("Starting...");

    let _ = run().await;
}
