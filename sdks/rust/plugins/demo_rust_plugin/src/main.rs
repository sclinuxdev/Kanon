//! Demo Rust Plugin Skeleton

use kanon_sdk::prelude::*;

#[allow(dead_code)]
struct DemoPlugin;

#[async_trait]
impl Plugin for DemoPlugin {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Demo Rust Plugin Skeleton");
    Ok(())
}
