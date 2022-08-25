use lazy_static::lazy_static;
use prometheus::{Encoder, Histogram, HistogramOpts, IntCounterVec, Opts, Registry};

use crate::error::Error;

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref PROCESSED_MESSAGES: IntCounterVec = IntCounterVec::new(
        Opts::new("processed_messages", "Processed Messages"),
        &["status"]
    )
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "processed_messages metric error");
        std::process::exit(1);
    });
    pub static ref PROCESSING_TIME: Histogram = Histogram::with_opts(HistogramOpts {
        common_opts: Opts::new("processing_time", "Event Processing Times"),
        buckets: vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 100.0],
    })
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "processing_time");
        std::process::exit(1);
    });
}

pub fn register_metrics() {
    REGISTRY
        .register(Box::new(PROCESSED_MESSAGES.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "processed_messages collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(PROCESSING_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "response_time collector error");
            std::process::exit(1);
        });
}

pub fn get_metrics() -> Result<String, Error> {
    let mut buffer = Vec::new();

    prometheus::TextEncoder::new()
        .encode(&REGISTRY.gather(), &mut buffer)
        .map_err(|e| e.to_string())?;

    let metrics = String::from_utf8(buffer).map_err(|e| e.to_string())?;
    Ok(metrics)
}
