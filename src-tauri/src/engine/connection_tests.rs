use super::test_support::*;
use crate::rpc::Rpc;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn ignores_events_from_a_replaced_codex_connection() {
    let (_d, e, _) = running_engine().await;
    let (old, _) = crate::rpc::tests::fixture().await;
    let (new, _) = crate::rpc::tests::fixture().await;
    *e.rpc.lock().await = Some(new.clone());
    let disconnect = |rpc: &Arc<Rpc>| {
        let (tx, rx) = mpsc::channel(1);
        tx.try_send(json!({"method":"orbit/disconnected"})).unwrap();
        e.clone().pump(rpc.clone(), rx)
    };
    disconnect(&old).await;
    assert_eq!(e.snapshot().await.reviews[0].status, "reviewing");
    disconnect(&new).await;
    assert_eq!(e.snapshot().await.reviews[0].status, "failed");
    old.shutdown().await;
    new.shutdown().await;
}
#[tokio::test]
async fn replacing_a_dead_connection_ends_its_reviews() {
    let (_d, e, _) = running_engine().await;
    let (dead, _) = crate::rpc::tests::fixture().await;
    dead.shutdown().await;
    *e.rpc.lock().await = Some(dead);
    e.retire_dead_connection().await;
    assert_eq!(e.snapshot().await.reviews[0].status, "failed");
}
