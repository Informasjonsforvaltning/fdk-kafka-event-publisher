use std::time::Instant;

use async_trait::async_trait;
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
use schema_registry_converter::async_impl::{avro::AvroEncoder, schema_registry::SrSettings};

use crate::{
    http::run_http_server,
    kafka::{send_event, BROKERS, SCHEMA_REGISTRY},
    metrics::{register_metrics, PROCESSED_MESSAGES, PROCESSING_TIME},
    schema::setup_schema,
};

pub mod error;
mod http;
pub mod kafka;
mod metrics;
mod rabbit;
mod schema;
pub mod utils;

lazy_static! {
    pub static ref PRODUCER: FutureProducer = kafka::create_producer().unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "kafka producer creation error");
        std::process::exit(1);
    });
    pub static ref SR_SETTINGS: SrSettings = create_sr_settings().unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "sr settings creation error");
        std::process::exit(1);
    });
}

pub struct ResourceConfig {
    pub consumer_name: String,
    pub routing_keys: Vec<String>,
}

#[derive(Clone)]
pub struct EventConfig {
    pub name: String,
    pub topic: String,
    pub schema: String,
}

#[async_trait]
pub trait Resource {
    type Event: kafka::Event + Send;

    async fn event(
        routing_key: &str,
        id: String,
        timestamp: i64,
        change: ChangeType,
    ) -> Result<Option<Self::Event>, Error>;
}

pub enum ChangeType {
    CreateOrUpdate,
    Remove,
}

pub async fn run_event_publisher<R: Resource + 'static>(
    resource_config: ResourceConfig,
    event_config: EventConfig,
) {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    tracing::info!(
        brokers = BROKERS.to_string(),
        schema_registry = SCHEMA_REGISTRY.to_string(),
        consumer_name = resource_config.consumer_name,
        output_topic = event_config.topic,
        routing_keys = format!("{:?}", resource_config.routing_keys),
        "starting service"
    );

    register_metrics();

    setup_schema(&SR_SETTINGS, &event_config)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "schema registration error");
            std::process::exit(1);
        });

    let channel = rabbit::connect().await.unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "rabbit connection error");
        std::process::exit(1);
    });
    rabbit::setup(
        &channel,
        &resource_config.consumer_name,
        &resource_config.routing_keys,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "rabbit setup error");
        std::process::exit(1);
    });
    let consumer = rabbit::create_consumer(&channel, &resource_config.consumer_name)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "rabbit consumer creation error");
            std::process::exit(1);
        });

    consumer.set_delegate(move |delivery| receive_message::<R>(event_config.clone(), delivery));

    run_http_server().await.unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "failed to run http server");
        std::process::exit(1);
    });
}

async fn receive_message<R: Resource>(event_config: EventConfig, delivery: DeliveryResult) {
    let delivery = match delivery {
        Ok(Some(delivery)) => delivery,
        Ok(None) => return,
        Err(error) => {
            tracing::error!(error = error.to_string(), "failed to consume message");
            std::process::exit(1);
        }
    };

    let start_time = Instant::now();
    let result =
        handle_message::<R>(&PRODUCER, SR_SETTINGS.clone(), &event_config, &delivery).await;
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
}

async fn handle_message<R: Resource>(
    producer: &FutureProducer,
    sr_settings: SrSettings,
    event_config: &EventConfig,
    delivery: &Delivery,
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
            tracing::debug!(id = resource.fdk_id.as_str(), "processing changed resource");

            if let Some(event) = R::event(
                delivery.routing_key.as_str(),
                resource.fdk_id,
                timestamp,
                ChangeType::CreateOrUpdate,
            )
            .await?
            {
                send_event(&mut encoder, &producer, &event_config, event).await?;
            };
        }

        if let Some(removed_resources) = element.removed_resources {
            for resource in removed_resources {
                tracing::debug!(id = resource.fdk_id.as_str(), "processing removed resource");

                if let Some(event) = R::event(
                    delivery.routing_key.as_str(),
                    resource.fdk_id,
                    timestamp,
                    ChangeType::Remove,
                )
                .await?
                {
                    send_event(&mut encoder, &producer, &event_config, event).await?;
                }
            }
        }
    }

    Ok(())
}
