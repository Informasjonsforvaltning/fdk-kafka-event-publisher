use std::env;

use async_trait::async_trait;
use lazy_static::lazy_static;
use serde_derive::Serialize;

use fdk_kafka_event_publisher::{
    error::Error, kafka, run_event_publisher, utils::http_get, ChangeType, EventConfig, Resource,
    ResourceConfig,
};

lazy_static! {
    static ref HARVESTER_API_URL: String =
        env::var("HARVESTER_API_URL").unwrap_or("http://localhost:8081".to_string());
    static ref REASONING_API_URL: String =
        env::var("REASONING_API_URL").unwrap_or("http://localhost:8082".to_string());
    static ref CONSUMER_NAME: String =
        env::var("CONSUMER_NAME").unwrap_or("fdk-information-model-event-publisher".to_string());
    static ref OUTPUT_TOPIC: String =
        env::var("OUTPUT_TOPIC").unwrap_or("information-model-events".to_string());
}

#[tokio::main]
async fn main() {
    let resource_config = ResourceConfig {
        consumer_name: CONSUMER_NAME.clone(),
        routing_keys: vec![
            "informationmodels.harvested".to_string(),
            "informationmodels.reasoned".to_string(),
        ],
    };

    let event_config = EventConfig {
        name: "no.fdk.informationmodel.InformationModelEvent".to_string(),
        topic: OUTPUT_TOPIC.clone(),
        schema: r#"{
                "name": "InformationModelEvent",
                "namespace": "no.fdk.informationmodels",
                "type": "record",
                "fields": [
                    {
                        "name": "type",
                        "type": {
                            "type": "enum",
                            "name": "InformationModelEventType",
                            "symbols": ["INFORMATION_MODEL_HARVESTED", "INFORMATION_MODEL_REASONED", "INFORMATION_MODEL_REMOVED"]
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
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    run_event_publisher::<InformationModel>(resource_config, event_config).await
}

pub struct InformationModel {}

#[async_trait]
impl Resource for InformationModel {
    type Event = InformationModelEvent;

    async fn event(
        routing_key: &str,
        id: String,
        timestamp: i64,
        report_change: ChangeType,
    ) -> Result<Option<Self::Event>, Error> {
        let event_type = match report_change {
            ChangeType::CreateOrUpdate => InformationModelEventType::from_routing_key(routing_key),
            ChangeType::Remove => Ok(InformationModelEventType::InformationModelRemoved),
        }?;

        let graph = match event_type {
            InformationModelEventType::InformationModelHarvested => {
                http_get(format!("{}/informationmodels/{}", HARVESTER_API_URL.as_str(), id)).await
            }
            InformationModelEventType::InformationModelReasoned => {
                http_get(format!("{}/information-models/{}", REASONING_API_URL.as_str(), id)).await
            }
            // Do not bother fetching graph for remove events
            InformationModelEventType::InformationModelRemoved => Ok("".to_string()),
        }?;

        Ok(Some(Self::Event {
            event_type,
            fdk_id: id,
            graph,
            timestamp,
        }))
    }
}

#[derive(Debug, Serialize)]
pub struct InformationModelEvent {
    #[serde(rename = "type")]
    pub event_type: InformationModelEventType,
    #[serde(rename = "fdkId")]
    pub fdk_id: String,
    pub graph: String,
    pub timestamp: i64,
}

impl kafka::Event for InformationModelEvent {
    fn key(&self) -> String {
        self.fdk_id.clone()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum InformationModelEventType {
    #[serde(rename = "INFORMATION_MODEL_HARVESTED")]
    InformationModelHarvested,
    #[serde(rename = "INFORMATION_MODEL_REASONED")]
    InformationModelReasoned,
    #[serde(rename = "INFORMATION_MODEL_REMOVED")]
    InformationModelRemoved,
}

impl InformationModelEventType {
    fn from_routing_key(routing_key: &str) -> Result<Self, Error> {
        match routing_key {
            "informationmodels.harvested" => Ok(Self::InformationModelHarvested),
            "informationmodels.reasoned" => Ok(Self::InformationModelReasoned),
            _ => Err(Error::String(format!(
                "unknown routing key: '{}'",
                routing_key
            ))),
        }
    }
}
