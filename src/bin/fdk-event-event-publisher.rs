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
    static ref CONSUMER_NAME: String =
        env::var("CONSUMER_NAME").unwrap_or("fdk-event-event-publisher".to_string());
    static ref OUTPUT_TOPIC: String =
        env::var("OUTPUT_TOPIC").unwrap_or("event-events".to_string());
}

#[tokio::main]
async fn main() {
    let resource_config = ResourceConfig {
        consumer_name: CONSUMER_NAME.clone(),
        routing_keys: vec![
            "events.harvested".to_string(),
        ],
    };

    let event_config = EventConfig {
        name: "no.fdk.event.EventEvent".to_string(),
        topic: OUTPUT_TOPIC.clone(),
        schema: r#"{
                "name": "EventEvent",
                "namespace": "no.fdk.event",
                "type": "record",
                "fields": [
                    {
                        "name": "type",
                        "type": {
                            "type": "enum",
                            "name": "EventEventType",
                            "symbols": ["EVENT_HARVESTED", "EVENT_REASONED", "EVENT_REMOVED"]
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

    run_event_publisher::<EventResource>(resource_config, event_config).await
}

pub struct EventResource {}

#[async_trait]
impl Resource for EventResource {
    type Event = EventEvent;

    async fn event(
        routing_key: &str,
        id: String,
        timestamp: i64,
        report_change: ChangeType,
    ) -> Result<Option<Self::Event>, Error> {
        let event_type = match report_change {
            ChangeType::CreateOrUpdate => EventEventType::from_routing_key(routing_key),
            ChangeType::Remove => Ok(EventEventType::EventRemoved),
        }?;

        let graph = match event_type {
            EventEventType::EventHarvested => {
                http_get(format!("{}/events/{}?catalogrecords=true", HARVESTER_API_URL.as_str(), id)).await
            }
            EventEventType::EventReasoned => Err(Error::String("should not handle reasoned messages".to_string())),
            // Do not bother fetching graph for remove events
            EventEventType::EventRemoved => Ok("".to_string()),
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
pub struct EventEvent {
    #[serde(rename = "type")]
    pub event_type: EventEventType,
    #[serde(rename = "fdkId")]
    pub fdk_id: String,
    pub graph: String,
    pub timestamp: i64,
}

impl kafka::Event for EventEvent {
    fn key(&self) -> String {
        self.fdk_id.clone()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum EventEventType {
    #[serde(rename = "EVENT_HARVESTED")]
    EventHarvested,
    #[serde(rename = "EVENT_REASONED")]
    EventReasoned,
    #[serde(rename = "EVENT_REMOVED")]
    EventRemoved,
}

impl EventEventType {
    fn from_routing_key(routing_key: &str) -> Result<Self, Error> {
        match routing_key {
            "events.harvested" => Ok(Self::EventHarvested),
            "events.reasoned" => Ok(Self::EventReasoned),
            _ => Err(Error::String(format!(
                "unknown routing key: '{}'",
                routing_key
            ))),
        }
    }
}
