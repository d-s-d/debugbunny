use http_body_util::BodyExt;
use reqwest::Url;

use crate::scrape_target::{FutureScrapeResult, ScrapeOk, ScrapeService};

pub struct HttpScrapeTarget {
    client: reqwest::Client,
    url: Url,
}

impl HttpScrapeTarget {
    pub fn new(client: &reqwest::Client, url: Url) -> Self {
        let client = client.clone();
        Self { client, url }
    }
}

impl ScrapeService for HttpScrapeTarget {
    type Response = ScrapeOk;
    fn call(&mut self) -> FutureScrapeResult<ScrapeOk> {
        let client = self.client.clone();
        let url = self.url.clone();
        // todo(dsd): Consider using hyper directly instead of reqwest.
        Box::pin(async move {
            // We want to fully materialize the response inside this method.
            // E.g., the outer timeout should also apply to reading the body,
            // and any open underlying response reader, etc. should be closed
            // before we return.
            let resp = client.get(url).send().await?;
            let (parts, body) = http::Response::from(resp).into_parts();
            let body = BodyExt::collect(body).await.map(|b| b.to_bytes())?.to_vec();
            Ok(ScrapeOk::HttpResponse(http::Response::from_parts(
                parts, body,
            )))
        })
    }
}
