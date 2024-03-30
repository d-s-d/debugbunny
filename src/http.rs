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
        Box::pin(async move { Ok(client.get(url).send().await.map(ScrapeOk::HttpResponse)?) })
    }
}
