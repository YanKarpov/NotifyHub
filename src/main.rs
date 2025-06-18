mod handlers;
mod worker;

use actix_web::{App, HttpServer, web};
use tracing::info;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::format::Writer;
use chrono::Local;
use std::fmt;

use dotenvy::dotenv;
use std::env;
use sqlx::PgPool;
use tokio::sync::broadcast;
use actix_files as fs;

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

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
        .with_target(true) 
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL где?");
    let db_pool = PgPool::connect(&database_url)
        .await
        .expect("Не удалось подключиться к базе данных");

    info!(target: "notifyhub", "Запуск сервера NotifyHub по адресу http://localhost:8080");

    let (tx, _rx) = broadcast::channel::<String>(100);

    let worker_count = 4;
    let mut counters = Vec::with_capacity(worker_count);

    for i in 0..worker_count {
        let counter = Arc::new(AtomicUsize::new(0));
        counters.push(counter.clone());

        let db_pool_clone = db_pool.clone();
        let tx_clone = tx.clone();
        let provider = worker::MockProvider;

        tokio::spawn(async move {
            info!(target: "notifyhub::worker", worker_id = i, "Worker started");
            worker::worker_loop_with_counter(
                i,
                db_pool_clone,
                provider,
                tx_clone,
                counter,
            )
            .await;
        });
    }

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .app_data(web::Data::new(tx.clone()))
            .service(handlers::enqueue)
            .service(handlers::health_check)
            .service(handlers::sse_events)
            .service(handlers::get_messages)
            .service(fs::Files::new("/", "./public").index_file("index.html"))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
