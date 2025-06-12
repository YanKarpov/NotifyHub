mod handlers;

use actix_web::{App, HttpServer};
use tracing::{info};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::format::Writer;
use chrono::Local;
use std::fmt;
use tokio::time::{sleep, Duration};
use std::sync::{Arc, Mutex};

struct SimpleTimer;

impl FormatTime for SimpleTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        let now = Local::now();
        write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S"))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_timer(SimpleTimer)
        .init();

    let queue: handlers::SharedQueue = Arc::new(Mutex::new(Vec::new()));
    let worker_queue = queue.clone();

    tokio::spawn(async move {
        loop {
            {
                let mut q = worker_queue.lock().unwrap();
                if let Some(msg) = q.pop() {
                    info!("Отправляем сообщение: provider={}, text={}", msg.provider, msg.text);
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
    });

    info!("Запуск сервера NotifyHub по адресу http://localhost:8080");

    HttpServer::new(move || {
        App::new()
            .app_data(actix_web::web::Data::new(queue.clone()))
            .service(handlers::enqueue)
            .service(handlers::health_check)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
