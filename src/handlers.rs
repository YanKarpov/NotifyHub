use actix_web::{post, get, web, HttpResponse, Responder};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

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
    actix_web::HttpResponse::Ok().body("OK")
}
