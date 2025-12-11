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
use serde_derive::Serialize;

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
        harvest_run_id: Option<String>,
        fdk_id: String,
        uri: Option<String>,
        timestamp: i64,
        change: ChangeType,
    ) -> Result<Option<Self::Event>, Error>;
}

#[derive(Debug)]
pub enum ChangeType {
    CreateOrUpdate,
    Remove,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum HarvestPhase {
    #[serde(rename = "INITIATING")]
    Initiating,
    #[serde(rename = "HARVESTING")]
    Harvesting,
    #[serde(rename = "REASONING")]
    Reasoning,
    #[serde(rename = "RDF_PARSING")]
    RdfParsing,
    #[serde(rename = "RESOURCE_PROCESSING")]
    ResourceProcessing,
    #[serde(rename = "SEARCH_PROCESSING")]
    SearchProcessing,
    #[serde(rename = "AI_SEARCH_PROCESSING")]
    AiSearchProcessing,
    #[serde(rename = "SPARQL_PROCESSING")]
    SparqlProcessing,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum DataType {
    #[serde(rename = "concept")]
    Concept,
    #[serde(rename = "dataset")]
    Dataset,
    #[serde(rename = "informationmodel")]
    InformationModel,
    #[serde(rename = "dataservice")]
    DataService,
    #[serde(rename = "publicService")]
    PublicService,
    #[serde(rename = "event")]
    Event,
}

#[derive(Debug, Serialize)]
pub struct HarvestEvent {
    pub phase: HarvestPhase,
    #[serde(rename = "dataSourceId")]
    pub data_source_id: String,
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "dataType")]
    pub data_type: DataType,
    #[serde(rename = "dataSourceUrl")]
    pub data_source_url: Option<String>,
    #[serde(rename = "acceptHeader")]
    pub accept_header: Option<String>,
    #[serde(rename = "fdkId")]
    pub fdk_id: Option<String>,
    #[serde(rename = "resourceUri")]
    pub resource_uri: Option<String>,
    pub timestamp: i64,
    #[serde(rename = "startTime")]
    pub start_time: Option<String>,
    #[serde(rename = "endTime")]
    pub end_time: Option<String>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
    #[serde(rename = "changedResourcesCount")]
    pub changed_resources_count: Option<i32>,
    #[serde(rename = "unchangedResourcesCount")]
    pub unchanged_resources_count: Option<i32>,
    #[serde(rename = "removedResourcesCount")]
    pub removed_resources_count: Option<i32>,
}

impl kafka::Event for HarvestEvent {
    fn key(&self) -> String {
        self.data_source_id.clone()
    }
}

fn routing_key_to_data_type(routing_key: &str) -> Option<DataType> {
    if routing_key.starts_with("concepts.") {
        Some(DataType::Concept)
    } else if routing_key.starts_with("datasets.") {
        Some(DataType::Dataset)
    } else if routing_key.starts_with("informationmodels.") {
        Some(DataType::InformationModel)
    } else if routing_key.starts_with("dataservices.") {
        Some(DataType::DataService)
    } else if routing_key.starts_with("public_services.") {
        Some(DataType::PublicService)
    } else if routing_key.starts_with("events.") {
        Some(DataType::Event)
    } else {
        None
    }
}

pub async fn run_event_publisher<R: Resource + 'static>(
    resource_config: ResourceConfig,
    event_config: EventConfig,
) {
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

    // Setup HarvestEvent schema
    let harvest_event_config = EventConfig {
        name: "no.fdk.harvest.HarvestEvent".to_string(),
        topic: std::env::var("HARVEST_EVENT_TOPIC").unwrap_or_else(|_| "harvest-events".to_string()),
        schema: include_str!("../kafka/schemas/no.fdk.harvest.HarvestEvent.avsc").to_string(),
    };
    setup_schema(&SR_SETTINGS, &harvest_event_config)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "harvest event schema registration error");
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

    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
        tracing::error!(error = e.to_string(), "failed to ack message");
    }
}

async fn handle_message<R: Resource>(
    producer: &FutureProducer,
    sr_settings: SrSettings,
    event_config: &EventConfig,
    delivery: &Delivery,
) -> Result<(), Error> {
    tracing::info!(
        routing_key = delivery.routing_key.as_str(),
        "received harvest report message"
    );

    let reports: Vec<HarvestReport> = serde_json::from_slice(&delivery.data)?;

    tracing::info!(
        routing_key = delivery.routing_key.as_str(),
        report_count = reports.len(),
        "parsed harvest reports"
    );

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

    tracing::debug!(
        routing_key = delivery.routing_key.as_str(),
        reports = format!("{:?}", reports),
        "processing event"
    );

    tracing::info!(
        routing_key = delivery.routing_key.as_str(),
        reports = reports.len(),
        changed_resource_count,
        removed_resource_count,
        "processing harvest reports"
    );
    let mut encoder = AvroEncoder::new(sr_settings);

    // HarvestEvent config (schema is already set up in run_event_publisher)
    let harvest_event_config = EventConfig {
        name: "no.fdk.harvest.HarvestEvent".to_string(),
        topic: std::env::var("HARVEST_EVENT_TOPIC").unwrap_or_else(|_| "harvest-events".to_string()),
        schema: include_str!("../kafka/schemas/no.fdk.harvest.HarvestEvent.avsc").to_string(),
    };

    for element in reports {
        let timestamp = DateTime::parse_from_str(&element.start_time, "%Y-%m-%d %H:%M:%S%.f %z")?
            .timestamp_millis();

        tracing::debug!(
            run_id = ?element.run_id,
            data_source_id = ?element.data_source_id,
            start_time = element.start_time.as_str(),
            end_time = ?element.end_time,
            error_message = ?element.error_message,
            changed_resources = element.changed_resources.len(),
            removed_resources = element.removed_resources.as_ref().map_or(0, |r| r.len()),
            "processing harvest report element"
        );

        // Produce HarvestEvent if report contains a runId
        if let Some(run_id) = &element.run_id {
            if let Some(data_type) = routing_key_to_data_type(delivery.routing_key.as_str()) {
                let data_source_id = element.data_source_id.clone().unwrap_or_else(|| "unknown".to_string());
                
                let changed_count = element.changed_resources.len() as i32;
                let removed_count = element.removed_resources.as_ref().map_or(0, |r| r.len()) as i32;

                tracing::info!(
                    run_id = run_id.as_str(),
                    data_source_id = data_source_id.as_str(),
                    data_type = ?data_type,
                    changed_count,
                    removed_count,
                    "producing harvest event"
                );

                let harvest_event = HarvestEvent {
                    phase: HarvestPhase::ResourceProcessing,
                    data_source_id: data_source_id.clone(),
                    run_id: run_id.clone(),
                    data_type,
                    data_source_url: None,
                    accept_header: None,
                    fdk_id: None,
                    resource_uri: None,
                    timestamp,
                    start_time: Some(element.start_time.clone()),
                    end_time: element.end_time.clone(),
                    error_message: element.error_message.clone(),
                    changed_resources_count: Some(changed_count),
                    unchanged_resources_count: Some(0),
                    removed_resources_count: Some(removed_count),
                };

                match send_event(&mut encoder, producer, &harvest_event_config, harvest_event).await {
                    Ok(_) => {
                        tracing::info!(
                            run_id = run_id.as_str(),
                            data_source_id = data_source_id.as_str(),
                            data_type = ?data_type,
                            "harvest event produced successfully"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            run_id = run_id.as_str(),
                            data_source_id = data_source_id.as_str(),
                            error = e.to_string(),
                            "failed to send harvest event"
                        );
                    }
                }
            } else {
                tracing::warn!(
                    routing_key = delivery.routing_key.as_str(),
                    run_id = run_id.as_str(),
                    "could not determine data type from routing key, skipping harvest event"
                );
            }
        } else {
            tracing::debug!(
                "harvest report element has no run_id, skipping harvest event production"
            );
        }

        tracing::debug!(
            run_id = ?element.run_id,
            changed_resources_count = element.changed_resources.len(),
            "processing changed resources from harvest report"
        );

        for resource in element.changed_resources {
            if let Err(e) = handle_event::<R>(
                &mut encoder,
                &producer,
                &event_config,
                delivery.routing_key.as_str(),
                element.run_id.clone(),
                resource.fdk_id.clone(),
                resource.uri.clone(),
                timestamp,
                ChangeType::CreateOrUpdate,
            )
            .await
            {
                tracing::error!(
                    harvest_run_id = ?element.run_id,
                    fdk_id = resource.fdk_id,
                    uri = ?resource.uri,
                    change = format!("{:?}", ChangeType::CreateOrUpdate),
                    error = e.to_string(),
                    "failed while handling event"
                );
            }
        }

        if let Some(removed_resources) = &element.removed_resources {
            tracing::debug!(
                run_id = ?element.run_id,
                removed_resources_count = removed_resources.len(),
                "processing removed resources from harvest report"
            );

            for resource in removed_resources {
                if let Err(e) = handle_event::<R>(
                    &mut encoder,
                    &producer,
                    &event_config,
                    delivery.routing_key.as_str(),
                    element.run_id.clone(),
                    resource.fdk_id.clone(),
                    resource.uri.clone(),
                    timestamp,
                    ChangeType::Remove,
                )
                .await
                {
                    tracing::error!(
                        id = resource.fdk_id,
                        uri = ?resource.uri,
                        change = format!("{:?}", ChangeType::Remove),
                        error = e.to_string(),
                        "failed while handling event"
                    );
                }
            }
        }
    }

    Ok(())
}

async fn handle_event<R: Resource>(
    mut encoder: &mut AvroEncoder<'_>,
    producer: &FutureProducer,
    event_config: &EventConfig,
    routing_key: &str,
    harvest_run_id: Option<String>,
    fdk_id: String,
    uri: Option<String>,
    timestamp: i64,
    change: ChangeType,
) -> Result<(), Error> {
    tracing::debug!(
        routing_key,
        fdk_id = fdk_id.as_str(),
        change = format!("{:?}", change),
        "processing event"
    );

    if let Some(event) = R::event(routing_key, harvest_run_id, fdk_id, uri, timestamp, change).await? {
        send_event(&mut encoder, &producer, &event_config, event).await?;
    };
    Ok(())
}
