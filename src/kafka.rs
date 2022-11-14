use std::{env, time::Duration};

use lazy_static::lazy_static;
use rdkafka::{
    producer::{FutureProducer, FutureRecord},
    ClientConfig,
};
use schema_registry_converter::{
    async_impl::{avro::AvroEncoder, schema_registry::SrSettings},
    schema_registry_common::SubjectNameStrategy,
};
use serde::Serialize;

use crate::EventConfig;

lazy_static! {
    pub static ref BROKERS: String = env::var("BROKERS").unwrap_or("localhost:9092".to_string());
    pub static ref SCHEMA_REGISTRY: String =
        env::var("SCHEMA_REGISTRY").unwrap_or("http://localhost:8081".to_string());
}

#[derive(Debug, thiserror::Error)]
pub enum KafkaError {
    #[error(transparent)]
    SRCError(#[from] schema_registry_converter::error::SRCError),
    #[error(transparent)]
    RdkafkaError(#[from] rdkafka::error::KafkaError),
}

pub trait Event: Serialize {
    fn key(&self) -> String;
}

pub async fn send_event<E: Event>(
    encoder: &mut AvroEncoder<'_>,
    producer: &FutureProducer,
    event_config: &EventConfig,
    event: E,
) -> Result<(), KafkaError> {
    let key = event.key();

    let encoded = encoder
        .encode_struct(
            event,
            &SubjectNameStrategy::RecordNameStrategy(event_config.name.to_string()),
        )
        .await?;

    let record = FutureRecord::to(&event_config.topic)
        .key(&key)
        .payload(&encoded);
    producer
        .send(record, Duration::from_secs(0))
        .await
        .map_err(|e| e.0)?;

    Ok(())
}

pub fn create_sr_settings() -> Result<SrSettings, KafkaError> {
    let mut schema_registry_urls = SCHEMA_REGISTRY.split(",");

    let mut sr_settings_builder =
        SrSettings::new_builder(schema_registry_urls.next().unwrap_or_default().to_string());
    schema_registry_urls.for_each(|url| {
        sr_settings_builder.add_url(url.to_string());
    });

    let sr_settings = sr_settings_builder
        .set_timeout(Duration::from_secs(5))
        .build()?;
    Ok(sr_settings)
}

pub fn create_producer() -> Result<FutureProducer, KafkaError> {
    let producer = ClientConfig::new()
        .set("bootstrap.servers", BROKERS.clone())
        .set("message.timeout.ms", "5000")
        .create()?;
    Ok(producer)
}
