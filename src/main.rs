mod handlers;
mod worker;

use actix_web::{App, HttpServer, web};
use tracing::{info};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::format::Writer;
use chrono::Local;
use std::fmt;

use dotenvy::dotenv;
use std::env;
use sqlx::PgPool;
use tokio::sync::broadcast;

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

    let (tx, _rx) = broadcast::channel::<String>(100);

    let db_pool_clone = db_pool.clone();
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        let provider = worker::MockProvider;
        worker::worker_loop(db_pool_clone, provider, tx_clone).await;
    });

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .app_data(web::Data::new(tx.clone()))
            .service(handlers::enqueue)
            .service(handlers::health_check)
            .service(handlers::sse_events)
            .service(handlers::get_messages)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
