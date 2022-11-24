use lapin::{
    options::{BasicConsumeOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, Connection, ConnectionProperties, Consumer,
};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum RabbitError {
    #[error(transparent)]
    LapinError(#[from] lapin::Error),
    #[error("{0}: {1}")]
    ConfigError(&'static str, String),
}

#[derive(Debug, Deserialize)]
pub struct HarvestReport {
    #[serde(alias = "startTime")]
    pub start_time: String,
    #[serde(alias = "changedResources")]
    pub changed_resources: Vec<HarvestReportChange>,
    #[serde(alias = "removedResources")]
    pub removed_resources: Option<Vec<HarvestReportChange>>,
}

#[derive(Debug, Deserialize)]
pub struct HarvestReportChange {
    #[serde(alias = "fdkId")]
    pub fdk_id: String,
}

fn var(key: &'static str) -> Result<String, RabbitError> {
    std::env::var(key).map_err(|e| RabbitError::ConfigError(key, e.to_string()))
}

fn connection_string() -> Result<String, RabbitError> {
    let user = var("RABBITMQ_USERNAME")?;
    let pass = var("RABBITMQ_PASSWORD")?;
    let host = var("RABBITMQ_HOST")?;
    let port = var("RABBITMQ_PORT")?;

    Ok(format!("amqp://{}:{}@{}:{}/%2f", user, pass, host, port))
}

pub async fn connect() -> Result<Channel, RabbitError> {
    let options = ConnectionProperties::default()
        .with_executor(tokio_executor_trait::Tokio::current())
        .with_reactor(tokio_reactor_trait::Tokio);

    let uri = connection_string()?;
    let connection = Connection::connect(&uri, options).await?;
    let channel = connection.create_channel().await?;
    Ok(channel)
}

pub async fn setup(
    channel: &Channel,
    consumer_name: &str,
    routing_keys: &Vec<String>,
) -> Result<(), RabbitError> {
    channel
        .queue_declare(
            consumer_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    for routing_key in routing_keys {
        channel
            .queue_bind(
                consumer_name,
                "harvests",
                routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;
    }

    Ok(())
}

pub async fn create_consumer(
    channel: &Channel,
    consumer_name: &str,
) -> Result<Consumer, RabbitError> {
    let consumer = channel
        .basic_consume(
            consumer_name,
            consumer_name,
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    Ok(consumer)
}
