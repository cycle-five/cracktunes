use crack_voting::run;

/// Main function
#[tokio::main]
async fn main() {
    println!("Starting server");
    run().await;
}
