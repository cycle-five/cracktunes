use crack_testing::run;

/// Main function
//#[tokio::main]
#[cfg(not(tarpaulin_include))]
fn main() {
    println!("Starting server");
    //let _ = run();
    run();
}
