# Tamarind load generator

This is a simple load generator that ceneters around fixed intervals and batch sizes. The  key mechanics leverage [`Tokio's`](https://docs.rs/tokio/latest/tokio/time/fn.interval.html) tick capability. Via the CLI, 
only RPS and a duration is supplied. HTTP/2 requests are generated against a (for now)
fixed [podinfo](https://github.com/stefanprodan/podinfo) service running in a kind cluster.

## Design
* Even though a user will supply requests per second (`rps`), work is only done in terms of fixed `100ms` buckets. This `100ms` represents the _resolution_ of our load generator. Conceptually, 1 RPS is `0.1` requests per 100ms bucket (1 request made every "10th" bucket). 10 RPS means 1 request is made at each of these `100ms` intervals. At 100 RPS; it's 10 requests per bucket and so on. If we keep track of the time deltas between when we make our requests, we can _discard_ that batch of work if we're lagging behind.
* These buckets (or batches) of work is calculated upfront and are sent along an `mpsc` channel even if there are "empty" buckets (at a low enough RPS, 1 request is made every Nth bucket).
* A channel receiver polls at every interval (`100ms`) and dispatches as many requests as the batch size in its own tokio task. Tasks are cheap and are not 1:1 with OS threads. Requests are made using the [hyper](https://github.com/hyperium/hyper-util/tree/master/src/server) client.
* Finally, this "producer" / "consumer" mechanism only runs for as long as the user-specified duration. This is due to tokio's [timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html)
that acts like a watch dog.

### Setup of http2 service in kind cluster
```
kind create cluster
kubectl create ns test
```
Thereafter, install the podinfo test service.
```
helm install podinfo-h2c-enabled -n test --set h2c.enabled=true podinfo/podinfo
```
Note that HTTP/2 is enabled via the h2c flag.
* Finally, port forward from local machine to the Kubernetes deployment:
```
kubectl -n test port-forward deploy/podinfo-h2c-enabled 8080:9898
```

### Running load generator locally

* On a different terminal run the following
```
cargo run -- --duration 10 --rps 1 
```
#### Useful commands
Edit number of pods
```
kubectl edit deployment -n test -o yaml
```

### Backlog
* [Med] Result reporting (e.g. error rate, latencies)
* [Med] Visualization (a cool plot of errors, latencies)
* [Low] Clean up types, structure and fix clippy lints


### Resources
* [hyper](https://hyper.rs/) (HTTP library)
* [clap](https://github.com/clap-rs/clap) (CLI tool for Rust)
* [tokio](https://tokio.rs/) (Async RUST runtime)