//! Sample Rust project for testing

mod utils;
mod models;

fn main() {
    let user = models::User::new("Alice");
    println!("Hello, {}!", user.name);
    
    let result = utils::calculate(1, 2);
    println!("Result: {}", result);
}
