use lazy_static::lazy_static;
use reqwest::StatusCode;

use crate::error::Error;

lazy_static! {
    static ref CLIENT: reqwest::Client =
        reqwest::ClientBuilder::new().build().unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "reqwest client creation error");
            std::process::exit(1);
        });
}

pub async fn http_get(url: String) -> Result<String, Error> {
    let response = CLIENT.get(url).send().await?;

    match response.status() {
        StatusCode::OK => Ok(response.text().await?),
        _ => Err(format!(
            "Invalid http response: {} - {}",
            response.status(),
            response.text().await?
        )
        .into()),
    }
}
