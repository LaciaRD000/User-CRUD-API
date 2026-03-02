mod db;
mod errors;
mod models;
mod routes;
mod snowflake;
mod state;
mod validation;

use dotenvy::dotenv;

fn main() {
    let _ = dotenv().expect(".env file not found");
}
