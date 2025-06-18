use actix_web::{post, get, web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use chrono::{Utc, NaiveDateTime};
use tokio::time::{interval, Duration};
use actix_web::web::Bytes;
use async_stream::stream;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info};

#[derive(Deserialize, Debug)]
pub struct Message {
    pub provider: String,
    pub text: String,
    pub scheduled_at: Option<NaiveDateTime>, 
}

#[derive(Serialize)]
struct MessageRecord {
    id: i32,
    provider: String,
    text: String,
    status: String,
    retry_count: i32,
    created_at: Option<chrono::NaiveDateTime>,
}

#[post("/enqueue")]
pub async fn enqueue(
    db_pool: web::Data<PgPool>,
    msg: web::Json<Message>,
) -> impl Responder {
    let now = Utc::now().naive_utc();

    let query = sqlx::query(
        "INSERT INTO messages (provider, text, status, created_at, next_retry_at)
         VALUES ($1, $2, 'pending', $3, $4)"
    )
    .bind(&msg.provider)
    .bind(&msg.text)
    .bind(now)
    .bind(msg.scheduled_at); 

    match query.execute(db_pool.get_ref()).await {
        Ok(_) => {
            info!("Message enqueued: provider={}, text={}", &msg.provider, &msg.text);
            HttpResponse::Ok().body("Message enqueued")
        },
        Err(e) => {
            error!("Failed to insert message: {}", e);
            HttpResponse::InternalServerError().body("Failed to enqueue message")
        }
    }
}

#[get("/health")]
pub async fn health_check() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[get("/events")]
pub async fn sse_events(tx: web::Data<broadcast::Sender<String>>) -> impl Responder {
    let mut rx = tx.subscribe();

    let event_stream = stream! {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let data = format!("data: {}\n\n", msg);
                    yield Ok::<Bytes, actix_web::Error>(Bytes::from(data));
                }
                Err(RecvError::Lagged(skipped)) => {
                    error!("Client lagged, skipped {} messages", skipped);
                }
                Err(RecvError::Closed) => break,
            }
        }
    };

    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream"))
        .insert_header(("Cache-Control", "no-cache"))
        .streaming(event_stream)
}

#[get("/messages")]
pub async fn get_messages(db_pool: web::Data<PgPool>) -> impl Responder {
    let rows = sqlx::query_as!(
        MessageRecord,
        r#"
        SELECT id, provider, text, status, retry_count, created_at
        FROM messages
        ORDER BY created_at DESC
        LIMIT 50
        "#
    )
    .fetch_all(db_pool.get_ref())
    .await;

    match rows {
        Ok(messages) => HttpResponse::Ok().json(messages),
        Err(e) => {
            error!("Failed to fetch messages: {}", e);
            HttpResponse::InternalServerError().body("Failed to fetch messages")
        }
    }
}
