# fdk-kafka-event-publisher

This is deployed as 6 different services:
- fdk-concept-event-publisher
- fdk-data-service-event-publisher
- fdk-dataset-event-publisher
- fdk-event-event-publisher
- fdk-information-model-event-publisher
- fdk-service-event-publisher

Each service connects the rabbit messages with harvest reports published by the harvesters to kafka. The harvest reports contain a list of changed resources and a list of removed resources, these are converted to kafka events.

Each item in the list of removed resources will result in a remove-event and each item in the list of changed resources will result in a harvest-event.

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
- `graph` The harvested graph of the resource, downloaded from the harvester for harvest events and is empty for remove events
