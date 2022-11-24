use std::{env, str::FromStr};

use async_trait::async_trait;
use lazy_static::lazy_static;
use serde_derive::Serialize;

use fdk_event_publisher::{
    error::Error, kafka, run_event_publisher, utils::http_get, ChangeType, EventConfig, Resource,
    ResourceConfig,
};

lazy_static! {
    static ref REASONING_API_URL: String =
        env::var("REASONING_API_URL").unwrap_or("http://localhost:8081".to_string());
    static ref CONSUMER_NAME: String =
        env::var("CONSUMER_NAME").unwrap_or("fdk-dataset-event-publisher".to_string());
    static ref OUTPUT_TOPIC: String =
        env::var("OUTPUT_TOPIC").unwrap_or("dataset-events".to_string());
}

#[tokio::main]
async fn main() {
    let resource_config = ResourceConfig {
        consumer_name: CONSUMER_NAME.clone(),
        routing_keys: vec![
            "datasets.harvested".to_string(),
            "datasets.reasoned".to_string(),
        ],
    };

    let event_config = EventConfig {
        name: "no.fdk.dataset.DatasetEvent".to_string(),
        topic: OUTPUT_TOPIC.clone(),
        schema: r#"{
                "name": "DatasetEvent",
                "namespace": "no.fdk.dataset",
                "type": "record",
                "fields": [
                    {
                        "name": "type",
                        "type": {
                            "type": "enum",
                            "name": "DatasetEventType",
                            "symbols": ["DATASET_HARVESTED", "DATASET_REASONED", "DATASET_REMOVED"]
                        }
                    },
                    {"name": "fdkId", "type": "string"},
                    {"name": "graph", "type": "string"},
                    {"name": "timestamp", "type": "long", "logicalType": "timestamp-millis"}
                ]
            }"#
        .to_string(),
    };

    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .with_current_span(false)
        .init();

    run_event_publisher::<Dataset>(resource_config, event_config).await
}

pub struct Dataset {}

#[async_trait]
impl Resource for Dataset {
    type Event = DatasetEvent;

    async fn event(
        routing_key: &str,
        id: String,
        timestamp: i64,
        report_change: ChangeType,
    ) -> Result<Option<Self::Event>, Error> {
        let (event_type, graph) = match report_change {
            ChangeType::CreateOrUpdate => (
                DatasetEventType::from_str(routing_key),
                http_get(format!("{}/datasets/{}", REASONING_API_URL.as_str(), id)).await,
            ),
            ChangeType::Remove => (
                Ok(DatasetEventType::DatasetRemoved),
                // Do not bother fetching graph for remove events
                Ok("".to_string()),
            ),
        };

        Ok(Some(Self::Event {
            event_type: event_type?,
            fdk_id: id,
            graph: graph?,
            timestamp,
        }))
    }
}

#[derive(Debug, Serialize)]
pub struct DatasetEvent {
    #[serde(rename = "type")]
    pub event_type: DatasetEventType,
    #[serde(rename = "fdkId")]
    pub fdk_id: String,
    pub graph: String,
    pub timestamp: i64,
}

impl kafka::Event for DatasetEvent {
    fn key(&self) -> String {
        self.fdk_id.clone()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum DatasetEventType {
    #[serde(rename = "DATASET_HARVESTED")]
    DatasetHarvested,
    #[serde(rename = "DATASET_REASONED")]
    DatasetReasoned,
    #[serde(rename = "DATASET_REMOVED")]
    DatasetRemoved,
}

impl FromStr for DatasetEventType {
    type Err = Error;

    fn from_str(routing_key: &str) -> Result<Self, Self::Err> {
        match routing_key {
            "datasets.harvested" => Ok(Self::DatasetHarvested),
            "datasets.reasoned" => Ok(Self::DatasetReasoned),
            _ => Err(Self::Err::String(format!(
                "unknown routing key received: '{}'",
                routing_key
            ))),
        }
    }
}
