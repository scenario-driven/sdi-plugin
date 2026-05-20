//! SSE endpoint. `GET /events` streams every `EventEnvelope` the broadcaster
//! emits, plus an initial `{"kind":"hello"}` so reconnecting clients know
//! the channel is alive.

use crate::state::{AppState, EventEnvelope};
use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::{self, Stream};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.events.subscribe();
    let hello = stream::once(async {
        Ok::<_, Infallible>(
            Event::default()
                .event("hello")
                .data(serde_json::to_string(&EventEnvelope {
                    kind: "hello".into(),
                    entity_id: None,
                    payload: serde_json::json!({"daemon": "sdid"}),
                })
                .unwrap_or_else(|_| "{}".into())),
        )
    });
    let live = BroadcastStream::new(rx).filter_map(|res| match res {
        Ok(env) => Some(Ok::<_, Infallible>(
            Event::default()
                .event(env.kind.as_str())
                .data(serde_json::to_string(&env).unwrap_or_else(|_| "{}".into())),
        )),
        Err(_lagged) => None,
    });
    Sse::new(hello.chain(live)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
