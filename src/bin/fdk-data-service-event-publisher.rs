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
        env::var("CONSUMER_NAME").unwrap_or("fdk-data-service-event-publisher".to_string());
    static ref OUTPUT_TOPIC: String =
        env::var("OUTPUT_TOPIC").unwrap_or("data-service-events".to_string());
}

#[tokio::main]
async fn main() {
    let resource_config = ResourceConfig {
        consumer_name: CONSUMER_NAME.clone(),
        routing_keys: vec![
            "dataservices.harvested".to_string(),
            "dataservices.reasoned".to_string(),
        ],
    };

    let event_config = EventConfig {
        name: "no.fdk.dataservice.DataServiceEvent".to_string(),
        topic: OUTPUT_TOPIC.clone(),
        schema: r#"{
                "name": "DataServiceEvent",
                "namespace": "no.fdk.dataservice",
                "type": "record",
                "fields": [
                    {
                        "name": "type",
                        "type": {
                            "type": "enum",
                            "name": "DataServiceEventType",
                            "symbols": ["DATA_SERVICE_HARVESTED", "DATA_SERVICE_REASONED", "DATA_SERVICE_REMOVED"]
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

    run_event_publisher::<DataService>(resource_config, event_config).await
}

pub struct DataService {}

#[async_trait]
impl Resource for DataService {
    type Event = DataServiceEvent;

    async fn event(
        routing_key: &str,
        id: String,
        timestamp: i64,
        report_change: ChangeType,
    ) -> Result<Option<Self::Event>, Error> {
        let event_type = match report_change {
            ChangeType::CreateOrUpdate => DataServiceEventType::from_routing_key(routing_key),
            ChangeType::Remove => Ok(DataServiceEventType::DataServiceRemoved),
        }?;

        let graph = match event_type {
            DataServiceEventType::DataServiceHarvested => {
                http_get(format!("{}/dataservices/{}", HARVESTER_API_URL.as_str(), id)).await
            }
            DataServiceEventType::DataServiceReasoned => {
                http_get(format!("{}/data-services/{}", REASONING_API_URL.as_str(), id)).await
            }
            // Do not bother fetching graph for remove events
            DataServiceEventType::DataServiceRemoved => Ok("".to_string()),
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
pub struct DataServiceEvent {
    #[serde(rename = "type")]
    pub event_type: DataServiceEventType,
    #[serde(rename = "fdkId")]
    pub fdk_id: String,
    pub graph: String,
    pub timestamp: i64,
}

impl kafka::Event for DataServiceEvent {
    fn key(&self) -> String {
        self.fdk_id.clone()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum DataServiceEventType {
    #[serde(rename = "DATA_SERVICE_HARVESTED")]
    DataServiceHarvested,
    #[serde(rename = "DATA_SERVICE_REASONED")]
    DataServiceReasoned,
    #[serde(rename = "DATA_SERVICE_REMOVED")]
    DataServiceRemoved,
}

impl DataServiceEventType {
    fn from_routing_key(routing_key: &str) -> Result<Self, Error> {
        match routing_key {
            "dataservices.harvested" => Ok(Self::DataServiceHarvested),
            "dataservices.reasoned" => Ok(Self::DataServiceReasoned),
            _ => Err(Error::String(format!(
                "unknown routing key: '{}'",
                routing_key
            ))),
        }
    }
}
