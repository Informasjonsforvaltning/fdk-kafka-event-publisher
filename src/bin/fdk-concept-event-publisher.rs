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
        env::var("CONSUMER_NAME").unwrap_or("fdk-concept-event-publisher".to_string());
    static ref OUTPUT_TOPIC: String =
        env::var("OUTPUT_TOPIC").unwrap_or("concept-events".to_string());
}

#[tokio::main]
async fn main() {
    let resource_config = ResourceConfig {
        consumer_name: CONSUMER_NAME.clone(),
        routing_keys: vec![
            "concepts.harvested".to_string(),
            "concepts.reasoned".to_string(),
        ],
    };

    let event_config = EventConfig {
        name: "no.fdk.concept.ConceptEvent".to_string(),
        topic: OUTPUT_TOPIC.clone(),
        schema: r#"{
                "name": "ConceptEvent",
                "namespace": "no.fdk.concept",
                "type": "record",
                "fields": [
                    {
                        "name": "type",
                        "type": {
                            "type": "enum",
                            "name": "ConceptEventType",
                            "symbols": ["CONCEPT_HARVESTED", "CONCEPT_REASONED", "CONCEPT_REMOVED"]
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

    run_event_publisher::<Concept>(resource_config, event_config).await
}

pub struct Concept {}

#[async_trait]
impl Resource for Concept {
    type Event = ConceptEvent;

    async fn event(
        routing_key: &str,
        id: String,
        timestamp: i64,
        report_change: ChangeType,
    ) -> Result<Option<Self::Event>, Error> {
        let event_type = match report_change {
            ChangeType::CreateOrUpdate => ConceptEventType::from_routing_key(routing_key),
            ChangeType::Remove => Ok(ConceptEventType::ConceptRemoved),
        }?;

        let graph = match event_type {
            ConceptEventType::ConceptHarvested => {
                http_get(format!("{}/concepts/{}", HARVESTER_API_URL.as_str(), id)).await
            }
            ConceptEventType::ConceptReasoned => {
                http_get(format!("{}/concepts/{}", REASONING_API_URL.as_str(), id)).await
            }
            // Do not bother fetching graph for remove events
            ConceptEventType::ConceptRemoved => Ok("".to_string()),
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
pub struct ConceptEvent {
    #[serde(rename = "type")]
    pub event_type: ConceptEventType,
    #[serde(rename = "fdkId")]
    pub fdk_id: String,
    pub graph: String,
    pub timestamp: i64,
}

impl kafka::Event for ConceptEvent {
    fn key(&self) -> String {
        self.fdk_id.clone()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum ConceptEventType {
    #[serde(rename = "CONCEPT_HARVESTED")]
    ConceptHarvested,
    #[serde(rename = "CONCEPT_REASONED")]
    ConceptReasoned,
    #[serde(rename = "CONCEPT_REMOVED")]
    ConceptRemoved,
}

impl ConceptEventType {
    fn from_routing_key(routing_key: &str) -> Result<Self, Error> {
        match routing_key {
            "concepts.harvested" => Ok(Self::ConceptHarvested),
            "concepts.reasoned" => Ok(Self::ConceptReasoned),
            _ => Err(Error::String(format!(
                "unknown routing key: '{}'",
                routing_key
            ))),
        }
    }
}
