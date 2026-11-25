//! when a node counts as ready to serve

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{Router, http::StatusCode, routing::get};
use tokio::{net::TcpListener, sync::watch, time::Instant};

use crate::web::ready::{self, Edge, Pace, Readiness, Stores};

/// the readiness after `probes`, and the edges they crossed, by probe number
fn run(probes: &[bool]) -> (Readiness, Vec<(usize, Edge)>) {
    let mut state = Readiness::START;
    let mut edges = Vec::new();
    for (number, passed) in probes.iter().enumerate() {
        let (next, edge) = state.step(*passed, &Pace::DEFAULT);
        state = next;
        edges.extend(edge.map(|edge| (number + 1, edge)));
    }
    (state, edges)
}

#[test]
fn a_node_is_ready_after_three_passes_in_a_row() {
    assert_eq!(run(&[true, true]), (Readiness::Down { passes: 2 }, vec![]));
    assert_eq!(
        run(&[true, true, true]),
        (Readiness::Up { fails: 0 }, vec![(3, Edge::Rose)])
    );
    assert_eq!(
        run(&[true, true, false, true, true, true]),
        (Readiness::Up { fails: 0 }, vec![(6, Edge::Rose)]),
        "a fail starts the count over"
    );
}

#[test]
fn a_ready_node_stops_after_five_fails_in_a_row() {
    let up = [true; 3];
    let flaky = [up.as_slice(), &[false; 4], &[true], &[false; 4]].concat();
    assert_eq!(
        run(&flaky),
        (Readiness::Up { fails: 4 }, vec![(3, Edge::Rose)]),
        "a pass starts the count over"
    );

    let gone = [up.as_slice(), &[false; 5]].concat();
    assert_eq!(
        run(&gone),
        (Readiness::START, vec![(3, Edge::Rose), (8, Edge::Fell)])
    );
}

/// a probe that answers with `verdicts` in turn, then passes; `None` hangs
fn scripted(
    verdicts: &[Option<bool>],
) -> impl FnMut() -> futures_util::future::BoxFuture<'static, Result<(), String>> {
    let verdicts = Arc::new(Mutex::new(
        verdicts.iter().copied().collect::<VecDeque<_>>(),
    ));
    move || {
        let verdict = verdicts.lock().unwrap().pop_front().unwrap_or(Some(true));
        Box::pin(async move {
            match verdict {
                Some(true) => Ok(()),
                Some(false) => Err("this node's loki does not answer".to_owned()),
                None => std::future::pending().await,
            }
        })
    }
}

/// when `ready` flips from what it holds, after `start`
async fn flip(ready: &mut watch::Receiver<bool>, start: Instant) -> (bool, Duration) {
    ready.changed().await.unwrap();
    (*ready.borrow_and_update(), start.elapsed())
}

#[tokio::test(start_paused = true)]
async fn readiness_is_published_on_its_edges_at_the_probe_interval() {
    let start = Instant::now();
    let (sender, mut ready) = watch::channel(false);
    let probe = scripted(&[
        Some(false),
        Some(true),
        Some(true),
        Some(true),
        None,
        Some(false),
        None,
        Some(false),
        Some(false),
    ]);
    tokio::spawn(ready::watch(Pace::DEFAULT, probe, sender));

    let second = Duration::from_secs(1);
    assert_eq!(
        flip(&mut ready, start).await,
        (true, 6 * second),
        "probes at 0, 2, 4 and 6 s, the first failing"
    );
    assert_eq!(
        flip(&mut ready, start).await,
        (false, 16 * second),
        "a probe that hangs fails once the interval is up"
    );
    assert_eq!(flip(&mut ready, start).await, (true, 22 * second));
}

#[tokio::test(start_paused = true)]
async fn readiness_stops_probing_once_nobody_listens() {
    let (sender, ready) = watch::channel(false);
    let watching = tokio::spawn(ready::watch(Pace::DEFAULT, scripted(&[]), sender));
    drop(ready);
    tokio::time::timeout(Duration::from_secs(5), watching)
        .await
        .unwrap()
        .unwrap();
}

/// a store on a port of its own that answers `status` where it says it is
/// ready
async fn store(status: StatusCode) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new().route("/-/ready", get(move || async move { status }));
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{address}/-/ready")
}

/// a port nothing listens on
async fn closed() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{address}/-/ready")
}

#[tokio::test]
async fn the_stores_pass_while_each_says_it_is_ready() {
    let stores = Stores::at(vec![
        ("prometheus", store(StatusCode::OK).await),
        ("loki", store(StatusCode::OK).await),
        ("alertmanager", store(StatusCode::OK).await),
    ]);
    assert_eq!(stores.answer().await, Ok(()));
}

#[tokio::test]
async fn a_store_that_is_down_or_not_ready_fails_the_probe() {
    let stopped = Stores::at(vec![
        ("prometheus", store(StatusCode::OK).await),
        ("loki", closed().await),
        ("alertmanager", store(StatusCode::OK).await),
    ]);
    assert_eq!(
        stopped.answer().await,
        Err("this node's loki does not answer".to_owned())
    );

    let starting = Stores::at(vec![
        ("prometheus", store(StatusCode::SERVICE_UNAVAILABLE).await),
        ("loki", store(StatusCode::OK).await),
    ]);
    assert_eq!(
        starting.answer().await,
        Err("this node's prometheus is not ready (503 Service Unavailable)".to_owned())
    );
}
