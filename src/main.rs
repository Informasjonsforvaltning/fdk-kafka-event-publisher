use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use error::Error;
use kafka::create_sr_settings;
use lapin::{message::DeliveryResult, options::BasicAckOptions};
use lazy_static::lazy_static;
use rabbit::HarvestReport;
use reqwest::StatusCode;
use schema_registry_converter::async_impl::avro::AvroEncoder;
use schemas::setup_schemas;

use crate::{kafka::send_event, schemas::DatasetEvent};

mod error;
mod kafka;
mod rabbit;
mod schemas;

lazy_static! {
    pub static ref HARVESTER_API_URL: String =
        env::var("HARVESTER_API_URL").unwrap_or("http://localhost:8080".to_string());
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    let sr_settings = create_sr_settings().unwrap();
    setup_schemas(&sr_settings).await.unwrap();

    let channel = rabbit::connect().await.expect("unable to connect");
    rabbit::setup(&channel)
        .await
        .expect("unable to setup rabbit queue");
    let consumer = rabbit::create_consumer(&channel)
        .await
        .expect("unable to create consumer");

    consumer.set_delegate(move |delivery: DeliveryResult| async move {
        let delivery = match delivery {
            Ok(Some(delivery)) => delivery,
            Ok(None) => return,
            Err(error) => {
                tracing::error!("failed to consume queue message {}", error);
                return;
            }
        };

        let producer = kafka::create_producer().unwrap();
        let client = reqwest::Client::new();
        let sr_settings = create_sr_settings().unwrap();
        let mut encoder = AvroEncoder::new(sr_settings);

        let report: Vec<HarvestReport> = serde_json::from_slice(&delivery.data).unwrap();
        for element in report {
            for resource in element.changed_resources {
                tracing::info!("id: {}", resource.fdk_id);
                let graph = get_graph(&client, &resource.fdk_id).await.unwrap().unwrap();
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                let message = DatasetEvent {
                    event_type: schemas::DatasetEventType::DatasetHarvested,
                    fdk_id: resource.fdk_id,
                    graph,
                    timestamp,
                };
                send_event(&mut encoder, &producer, message).await.unwrap();
            }
        }

        delivery
            .ack(BasicAckOptions::default())
            .await
            .expect("Failed to ack send_webhook_event message");
    });

    tokio::time::sleep(tokio::time::Duration::MAX).await;
}

async fn get_graph(client: &reqwest::Client, id: &String) -> Result<Option<String>, Error> {
    let response = client
        .get(format!("{}/datasets/{}", HARVESTER_API_URL.clone(), id))
        .send()
        .await?;

    match response.status() {
        StatusCode::NOT_FOUND => Ok(None),
        StatusCode::OK => Ok(Some(response.text().await?)),
        _ => Err(format!(
            "Invalid response from harvester: {} - {}",
            response.status(),
            response.text().await?
        )
        .into()),
    }
}
