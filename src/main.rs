use std::{env, time::Instant};

use actix_web::{get, App, HttpServer, Responder};
use chrono::DateTime;
use error::Error;
use kafka::create_sr_settings;
use lapin::{
    message::{Delivery, DeliveryResult},
    options::BasicAckOptions,
};
use lazy_static::lazy_static;
use rabbit::HarvestReport;
use rdkafka::producer::FutureProducer;
use reqwest::StatusCode;
use schema_registry_converter::async_impl::{avro::AvroEncoder, schema_registry::SrSettings};
use schemas::setup_schemas;

use crate::{
    kafka::{send_event, BROKERS, OUTPUT_TOPIC, SCHEMA_REGISTRY},
    metrics::{get_metrics, register_metrics, PROCESSED_MESSAGES, PROCESSING_TIME},
    schemas::DatasetEvent,
};

mod error;
mod kafka;
mod metrics;
mod rabbit;
mod schemas;

lazy_static! {
    pub static ref HARVESTER_API_URL: String =
        env::var("HARVESTER_API_URL").unwrap_or("http://localhost:8080".to_string());
    pub static ref REASONING_API_URL: String =
        env::var("REASONING_API_URL").unwrap_or("http://localhost:8081".to_string());
    pub static ref CONSUMER_NAME: String =
        env::var("CONSUMER_NAME").unwrap_or("fdk-dataset-event-publisher".to_string());
    pub static ref ROUTING_KEYS: Vec<String> = env::var("ROUTING_KEY")
        .map(|s| s.split(",").map(|s| s.to_string()).collect())
        .unwrap_or(vec![
            "datasets.harvested".to_string(),
            "datasets.reasoned".to_string()
        ]);
    pub static ref PRODUCER: FutureProducer = kafka::create_producer().unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "kafka producer creation error");
        std::process::exit(1);
    });
    pub static ref CLIENT: reqwest::Client =
        reqwest::ClientBuilder::new().build().unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "reqwest client creation error");
            std::process::exit(1);
        });
    pub static ref SR_SETTINGS: SrSettings = create_sr_settings().unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "sr settings creation error");
        std::process::exit(1);
    });
}

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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    register_metrics();

    tracing::info!(
        brokers = BROKERS.to_string(),
        schema_registry = SCHEMA_REGISTRY.to_string(),
        output_topic = OUTPUT_TOPIC.to_string(),
        harvester_api_url = HARVESTER_API_URL.to_string(),
        consumer_name = CONSUMER_NAME.to_string(),
        routing_keys = format!("{:?}", ROUTING_KEYS.clone()),
        "starting service"
    );

    setup_schemas(&SR_SETTINGS).await.unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "schema registration error");
        std::process::exit(1);
    });

    let channel = rabbit::connect().await.unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "rabbit connection error");
        std::process::exit(1);
    });
    rabbit::setup(&channel, &CONSUMER_NAME, &ROUTING_KEYS)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "rabbit setup error");
            std::process::exit(1);
        });
    let consumer = rabbit::create_consumer(&channel, &CONSUMER_NAME)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "rabbit consumer creation error");
            std::process::exit(1);
        });

    consumer.set_delegate(move |delivery: DeliveryResult| async {
        let delivery = match delivery {
            Ok(Some(delivery)) => delivery,
            Ok(None) => return,
            Err(error) => {
                tracing::error!(error = error.to_string(), "failed to consume message");
                std::process::exit(1);
            }
        };

        let start_time = Instant::now();
        let result = handle_message(&PRODUCER, &CLIENT, SR_SETTINGS.clone(), &delivery).await;
        let elapsed_millis = start_time.elapsed().as_millis();

        let metric_status_label = match result {
            Ok(_) => {
                tracing::info!(elapsed_millis, "message handled successfully");
                "success"
            }
            Err(e) => {
                tracing::error!(
                    elapsed_millis,
                    error = e.to_string(),
                    "failed while handling message"
                );
                "error"
            }
        };
        PROCESSED_MESSAGES
            .with_label_values(&[metric_status_label])
            .inc();
        PROCESSING_TIME.observe(elapsed_millis as f64 / 1000.0);

        delivery
            .ack(BasicAckOptions::default())
            .await
            .unwrap_or_else(|e| tracing::error!(error = e.to_string(), "failed to ack message"));
    });

    HttpServer::new(|| {
        App::new()
            .service(ping)
            .service(ready)
            .service(metrics_service)
    })
    .bind(("0.0.0.0", 8080))
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "metrics server error");
        std::process::exit(1);
    })
    .run()
    .await
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "failed to run metrics server");
        std::process::exit(1);
    });
}

async fn handle_message(
    producer: &FutureProducer,
    client: &reqwest::Client,
    sr_settings: SrSettings,
    delivery: &Delivery,
) -> Result<(), Error> {
    match delivery.routing_key.as_str() {
        "datasets.harvested" => {
            handle_dataset_message(
                producer,
                client,
                sr_settings,
                delivery,
                HARVESTER_API_URL.as_str(),
                schemas::DatasetEventType::DatasetHarvested,
            )
            .await
        }
        "datasets.reasoned" => {
            handle_dataset_message(
                producer,
                client,
                sr_settings,
                delivery,
                REASONING_API_URL.as_str(),
                schemas::DatasetEventType::DatasetReasoned,
            )
            .await
        }
        _ => {
            tracing::error!(
                routing_key = delivery.routing_key.as_str(),
                "unknown event received"
            );
            Ok(())
        }
    }
}

async fn handle_dataset_message(
    producer: &FutureProducer,
    client: &reqwest::Client,
    sr_settings: SrSettings,
    delivery: &Delivery,
    base_url: &str,
    event_type: schemas::DatasetEventType,
) -> Result<(), Error> {
    let reports: Vec<HarvestReport> = serde_json::from_slice(&delivery.data)?;
    let changed_resource_count = reports
        .iter()
        .map(|element| element.changed_resources.len())
        .sum::<usize>();
    let removed_resource_count = reports
        .iter()
        .map(|element| {
            element
                .removed_resources
                .as_ref()
                .map_or(0, |resources| resources.len())
        })
        .sum::<usize>();

    tracing::info!(
        routing_key = delivery.routing_key.as_str(),
        reports = reports.len(),
        changed_resource_count,
        removed_resource_count,
        "processing event"
    );
    let mut encoder = AvroEncoder::new(sr_settings);

    for element in reports {
        let timestamp = DateTime::parse_from_str(&element.start_time, "%Y-%m-%d %H:%M:%S%.f %z")?
            .timestamp_millis();

        for resource in element.changed_resources {
            tracing::debug!(id = resource.fdk_id.as_str(), "processing changed dataset");
            if let Some(graph) = get_graph(&client, base_url, &resource.fdk_id).await? {
                let message = DatasetEvent {
                    event_type,
                    fdk_id: resource.fdk_id,
                    graph,
                    timestamp,
                };

                send_event(&mut encoder, &producer, message).await?;
            } else {
                tracing::error!(id = resource.fdk_id, "graph not found");
            }
        }

        if let Some(removed_resources) = element.removed_resources {
            for resource in removed_resources {
                tracing::debug!(id = resource.fdk_id.as_str(), "processing removed dataset");
                let message = DatasetEvent {
                    event_type: schemas::DatasetEventType::DatasetRemoved,
                    fdk_id: resource.fdk_id,
                    // TODO: this should probably not be empty string
                    graph: "".to_string(),
                    timestamp,
                };

                send_event(&mut encoder, &producer, message).await?;
            }
        }
    }

    Ok(())
}

async fn get_graph(
    client: &reqwest::Client,
    base_url: &str,
    id: &String,
) -> Result<Option<String>, Error> {
    let response = client
        .get(format!("{}/datasets/{}", base_url, id))
        .send()
        .await?;

    match response.status() {
        StatusCode::NOT_FOUND => Ok(None),
        StatusCode::OK => Ok(Some(response.text().await?)),
        _ => Err(format!(
            "Invalid response from harvester: {} - {}",
            response.status(),
            response.text().await?
        )
        .into()),
    }
}
