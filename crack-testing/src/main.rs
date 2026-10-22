use crack_testing::run;

/// Main function
#[tokio::main]
async fn main() {
    println!("Starting...");
    let _ = run().await;
}
