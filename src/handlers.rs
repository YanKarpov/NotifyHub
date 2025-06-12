use actix_web::{post, get, web, HttpResponse, Responder};
use serde::Deserialize;
use sqlx::PgPool;
use chrono::Utc;
use tokio::time::{interval, Duration};
use actix_web::web::Bytes;
use async_stream::stream;

#[derive(Deserialize, Debug)]
pub struct Message {
    pub provider: String,
    pub text: String,
}

#[post("/enqueue")]
pub async fn enqueue(
    db_pool: web::Data<PgPool>,
    msg: web::Json<Message>,
) -> impl Responder {
    let now = Utc::now().naive_utc();  

    let query = sqlx::query(
        "INSERT INTO messages (provider, text, status, created_at) VALUES ($1, $2, 'pending', $3)"
    )
    .bind(&msg.provider)
    .bind(&msg.text)
    .bind(now);

    match query.execute(db_pool.get_ref()).await {
        Ok(_) => HttpResponse::Ok().body("Message enqueued"),
        Err(e) => {
            eprintln!("Failed to insert message: {}", e);
            HttpResponse::InternalServerError().body("Failed to enqueue message")
        }
    }
}

#[get("/health")]
pub async fn health_check() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[get("/events")]
pub async fn sse_events() -> impl Responder {
    let mut interval = interval(Duration::from_secs(1));

    let event_stream = stream! {
        loop {
            interval.tick().await;

            let data = format!(
                "data: {{\"event\":\"heartbeat\", \"time\":\"{}\"}}\n\n",
                Utc::now().format("%Y-%m-%d %H:%M:%S")
            );

            yield Ok::<Bytes, actix_web::Error>(Bytes::from(data));
        }
    };

    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream"))
        .insert_header(("Cache-Control", "no-cache"))
        .streaming(event_stream)
}
