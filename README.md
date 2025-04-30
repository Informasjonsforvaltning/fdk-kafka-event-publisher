# FDK Kafka Event Publisher

This application consumes RabbitMQ messages from harvesters and publishes messages to Kafka.

For a broader understanding of the system’s context, refer to
the [architecture documentation](https://github.com/Informasjonsforvaltning/architecture-documentation) wiki. For more
specific context on this application, see the **Harvesting** subsystem section.

This application is deployed as 6 different services:

- fdk-concept-event-publisher
- fdk-data-service-event-publisher
- fdk-dataset-event-publisher
- fdk-event-event-publisher
- fdk-information-model-event-publisher
- fdk-service-event-publisher

Each service connects the rabbit messages with harvest reports published by the harvesters to kafka. The harvest reports
contain a list of changed resources and a list of removed resources, these are converted to kafka events.

Each item in the list of removed resources will result in a remove-event and each item in the list of changed resources
will result in a harvest-event.

The produced kafka events:

- `CONCEPT_HARVESTED` & `CONCEPT_REMOVED`
- `DATA_SERVICE_HARVESTED` & `DATA_SERVICE_REMOVED`
- `DATASET_HARVESTED` & `DATASET_REMOVED`
- `EVENT_HARVESTED` & `EVENT_REMOVED`
- `INFORMATION_MODEL_HARVESTED` & `INFORMATION_MODEL_REMOVED`
- `SERVICE_HARVESTED` & `SERVICE_REMOVED`

Each event contain these parameters:

- `fdkId` The fdkId of the resource
- `timestamp` The timestamp when the harvest started, parsed from the `startTime` field in the harvest report
- `graph` The harvested graph of the resource, downloaded from the harvester for harvest events and is empty for remove
  events

## Getting Started

These instructions will give you a copy of the project up and running on your local machine for development and testing
purposes.

### Prerequisites

Ensure you have the following installed:

- [Rust](https://www.rust-lang.org/tools/install)
- [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- [Docker](https://docs.docker.com/get-docker/)
- [Docker Compose](https://docs.docker.com/compose/install/)

### Running locally

#### Clone the repository:

```sh
git clone https://github.com/Informasjonsforvaltning/fdk-kafka-event-publisher.git
cd fdk-kafka-event-publisher
```

Build for development:

```sh
cargo build --verbose
```

#### Start Kafka cluster and setup topics/schemas

Topics and schemas are set up automatically when starting the Kafka cluster. Docker compose uses the scripts
```create-topics.sh``` and ```create-schemas.sh``` to set up topics and schemas.

```sh
docker-compose up -d
```

#### Start application (choose a publisher from ```bin```):

```sh
RABBITMQ_HOST=localhost RABBITMQ_PORT=5672 RABBITMQ_USERNAME=guest RABBITMQ_PASSWORD=guest cargo run --bin fdk-dataset-event-publisher
```
