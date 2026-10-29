// Part 1 of tests/search.rs, included there, so the test names stay unchanged.

#[derive(Clone, Default)]
struct Provider {
    calls: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    started: Arc<Notify>,
    resume: Arc<Notify>,
}

async fn embeddings(
    State(p): State<Provider>,
    Json(v): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    p.calls.fetch_add(1, Ordering::SeqCst);
    if p.pause.swap(false, Ordering::SeqCst) {
        p.started.notify_one();
        p.resume.notified().await;
    }
    if p.fail.load(Ordering::SeqCst) {
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
    }
    assert_eq!(v["model"], joppa::embeddings::MODEL);
    let data = v["input"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .rev()
        .map(|(index, s)| {
            let s = s.as_str().unwrap().to_lowercase();
            let mut vector = vec![0.0; 4096];
            let dimension = if s.contains("renewal") || s.contains("extend membership") {
                0
            } else if s.contains("invoice") {
                1
            } else {
                2
            };
            vector[dimension] = 1.0;
            json!({"index":index,"embedding":vector})
        })
        .collect::<Vec<_>>();
    (axum::http::StatusCode::OK, Json(json!({"data":data})))
}
