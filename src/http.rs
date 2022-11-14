use actix_web::{get, App, HttpServer, Responder};

use crate::metrics::get_metrics;

#[get("/ping")]
async fn ping() -> impl Responder {
    "pong"
}

#[get("/ready")]
async fn ready() -> impl Responder {
    "ok"
}

#[get("/metrics")]
async fn metrics_service() -> impl Responder {
    match get_metrics() {
        Ok(metrics) => metrics,
        Err(e) => {
            tracing::error!(error = e.to_string(), "unable to gather metrics");
            "".to_string()
        }
    }
}

// TODO: should maybe return Server struct instead of a future?
pub async fn run_http_server() -> Result<(), std::io::Error> {
    HttpServer::new(|| {
        App::new()
            .service(ping)
            .service(ready)
            .service(metrics_service)
    })
    .bind(("0.0.0.0", 8080))
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "http server error");
        std::process::exit(1);
    })
    .run()
    .await
}
