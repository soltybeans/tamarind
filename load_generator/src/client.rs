use http_body_util::{BodyExt, Full};
use std::time::{Instant, Duration};
use hyper::{Method, Request, body::Bytes};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use hyper_util::client::legacy::connect::HttpConnector;

pub async fn make_request(uri: String, client: Client<HttpConnector, Full<Bytes>>) {
    let request: Request<Full<Bytes>> = Request::builder()
        .method(Method::GET)
        .uri(format!("http://{}", uri))
        .body(Full::from(" "))
        .expect("error making this request");

    let start_time = Instant::now();
    let response_future = client.request(request);
    let response = response_future.await.unwrap();
    let (_, body) = response.into_parts();

    let _ = body.collect().await.unwrap();
    let _elapsed_time_millis = start_time.elapsed().as_millis();
}

pub async fn make_client() -> Client<HttpConnector, Full<Bytes>> {
    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(5))
        .pool_timer(TokioTimer::new())
        .http2_only(true)
        .build_http()
} 