use schema_registry_converter::{
    async_impl::schema_registry::{post_schema, SrSettings},
    schema_registry_common::{SchemaType, SuppliedSchema},
};

use crate::{kafka::KafkaError, EventConfig};

pub async fn setup_schema(
    sr_settings: &SrSettings,
    event_config: &EventConfig,
) -> Result<(), KafkaError> {
    tracing::info!(event_config.name, "registering schema");

    let schema = post_schema(
        sr_settings,
        event_config.name.to_string(),
        SuppliedSchema {
            name: Some(event_config.name.to_string()),
            schema_type: SchemaType::Avro,
            schema: event_config.schema.to_string(),
            references: vec![],
        },
    )
    .await?;

    tracing::info!(
        id = schema.id,
        event_config.name,
        "schema succesfully registered"
    );
    Ok(())
}
