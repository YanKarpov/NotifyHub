mod handlers;

use actix_web::{App, HttpServer, web};
use tracing::{info};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::format::Writer;
use chrono::Local;
use std::fmt;

use dotenvy::dotenv;
use std::env;
use sqlx::PgPool;

struct SimpleTimer;

impl FormatTime for SimpleTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        let now = Local::now();
        write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S"))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok(); 
    tracing_subscriber::fmt()
        .with_timer(SimpleTimer)
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL где?");
    let db_pool = PgPool::connect(&database_url)
        .await
        .expect("Не удалось подключиться к базе данных");

    info!("Запуск сервера NotifyHub по адресу http://localhost:8080");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .service(handlers::enqueue)
            .service(handlers::health_check)
            .service(handlers::sse_events)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
