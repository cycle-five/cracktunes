use crack_testing::reply_handle_trait::run;

/// Main function
#[tokio::main]
#[cfg(not(tarpaulin_include))]
async fn main() {
    println!("Starting server");
    let _ = run().await;
    // run();
}
