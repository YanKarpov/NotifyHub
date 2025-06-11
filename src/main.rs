use actix_web::{get, App, HttpResponse, HttpServer, Responder};
use tracing::{info};
use tracing_subscriber;

#[get("/health")]
async fn health_check() -> impl Responder {
    info!("Вызван эндпоинт проверки здоровья сервера");
    HttpResponse::Ok().body("OK")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();

    info!("Запуск сервера NotifyHub по адресу http://localhost:8080");

    HttpServer::new(|| {
        App::new()
            .service(health_check)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
