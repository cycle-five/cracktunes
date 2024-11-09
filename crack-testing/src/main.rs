use crack_testing::run;

/// Main function
#[tokio::main]
#[cfg(not(tarpaulin_include))]
async fn main() {
    println!("Starting...");
    let _ = run().await;
}
