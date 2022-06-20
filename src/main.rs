use lapin::{
    message::DeliveryResult,
    options::{BasicAckOptions, BasicConsumeOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Connection, ConnectionProperties,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct HarvestReport {
    #[serde(alias = "changedResources")]
    changed_resources: Vec<HarvestReportChange>,
}

#[derive(Deserialize)]
pub struct HarvestReportChange {
    #[serde(alias = "fdkId")]
    fdk_id: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    let username = "admin".to_string();
    let password = "admin".to_string();
    let host = "127.0.0.1".to_string();
    let port = 5672;
    let uri = format!("amqp://{}:{}@{}:{}/%2f", username, password, host, port);

    let options = ConnectionProperties::default()
        .with_executor(tokio_executor_trait::Tokio::current())
        .with_reactor(tokio_reactor_trait::Tokio);

    let connection = Connection::connect(&uri, options).await.unwrap();
    let channel = connection.create_channel().await.unwrap();

    let _queue = channel
        .queue_declare(
            "fdk-dataset-event-publisher",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("unable to create queue");

    channel
        .queue_bind(
            "fdk-dataset-event-publisher",
            "harvests",
            "datasets.harvested",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("unable to bind excange to queue");

    let consumer = channel
        .basic_consume(
            "fdk-dataset-event-publisher",
            "fdk-dataset-event-publisher",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("unable to create queue consumer");

    consumer.set_delegate(move |delivery: DeliveryResult| async move {
        let delivery = match delivery {
            Ok(Some(delivery)) => delivery,
            Ok(None) => return,
            Err(error) => {
                dbg!("Failed to consume queue message {}", error);
                return;
            }
        };

        let report: Vec<HarvestReport> = serde_json::from_slice(&delivery.data).unwrap();
        for element in report {
            for resource in element.changed_resources {
                println!("{}", resource.fdk_id);
            }
        }

        delivery
            .ack(BasicAckOptions::default())
            .await
            .expect("Failed to ack send_webhook_event message");
    });

    tokio::time::sleep(tokio::time::Duration::MAX).await;
}
