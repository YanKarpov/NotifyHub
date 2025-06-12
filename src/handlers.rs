use actix_web::{post, get, web, HttpResponse, Responder};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::interval;
use actix_web::web::Bytes;
use chrono::Local;
use async_stream::stream;

#[derive(Deserialize, Debug)]
pub struct Message {
    pub provider: String,
    pub text: String,
}

pub type SharedQueue = Arc<Mutex<Vec<Message>>>;

#[post("/enqueue")]
pub async fn enqueue(
    queue: web::Data<SharedQueue>,
    msg: web::Json<Message>,
) -> impl Responder {
    let mut q = queue.lock().unwrap();
    q.push(msg.into_inner());
    HttpResponse::Ok().body("Message enqueued")
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
                Local::now().format("%Y-%m-%d %H:%M:%S")
            );

            yield Ok::<Bytes, actix_web::Error>(Bytes::from(data));
        }
    };

    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream"))
        .insert_header(("Cache-Control", "no-cache"))
        .streaming(event_stream)
}
