use std::str::FromStr;

use schema_registry_converter::{
    async_impl::schema_registry::{post_schema, SrSettings},
    schema_registry_common::{SchemaType, SuppliedSchema},
};
use serde_derive::Serialize;

use crate::{error::Error, kafka::KafkaError};

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

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "datasets.harvested" => Ok(Self::DatasetHarvested),
            "datasets.reasoned" => Ok(Self::DatasetReasoned),
            _ => Err(Self::Err::String(format!(
                "unknown event (routing key) received: '{}'",
                s
            ))),
        }
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

pub async fn setup_schemas(sr_settings: &SrSettings) -> Result<(), KafkaError> {
    register_schema(
        sr_settings,
        "no.fdk.dataset.DatasetEvent",
        r#"{
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
        }"#,
    )
    .await?;
    Ok(())
}

pub async fn register_schema(
    sr_settings: &SrSettings,
    name: &str,
    schema_str: &str,
) -> Result<(), KafkaError> {
    tracing::info!(name, "registering schema");

    let schema = post_schema(
        sr_settings,
        name.to_string(),
        SuppliedSchema {
            name: Some(name.to_string()),
            schema_type: SchemaType::Avro,
            schema: schema_str.to_string(),
            references: vec![],
        },
    )
    .await?;

    tracing::info!(id = schema.id, name, "schema succesfully registered");
    Ok(())
}
