#![cfg(target_os = "macos")]

use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use foundryui_lib::foundry::{embeddings, local_inference};

#[tokio::test]
async fn embedding_context_is_not_transmitted_after_the_backend_session_changes() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let address = listener.local_addr().expect("address");
    let revision = Arc::new(AtomicU64::new(1));
    let binding = local_inference::RuntimeBinding::default();
    binding.replace(Some(
        local_inference::RuntimeObservation::new(
            &format!("http://{address}"),
            std::process::id(),
            std::env::current_exe().expect("fixture executable"),
            1,
        )
        .expect("fixture observation"),
    ));
    let endpoint = local_inference::VerifiedLocalEndpoint::verify(&binding)
        .expect("native fixture proof")
        .bind_session(revision.clone(), 1);
    revision.store(2, Ordering::SeqCst);

    let error = embeddings::embed_texts(
        &endpoint,
        "fixture-model",
        &["private-fixture-content".to_string()],
    )
    .await
    .expect_err("changed session must block context");

    assert_eq!(error.code(), "CommandFailed");
    assert!(
        !serde_json::to_string(&error)
            .expect("error JSON")
            .contains("private-fixture-content")
    );
    assert_eq!(
        listener.accept().expect_err("no connection").kind(),
        std::io::ErrorKind::WouldBlock
    );
}
