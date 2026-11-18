use super::{LocalInferenceError, RuntimeBinding, RuntimeObservation};

#[cfg(target_os = "macos")]
#[test]
fn cancelled_dispatch_waiting_for_native_worker_never_opens_tcp() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let proof = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let blocker = runtime.spawn_blocking(move || {
        started_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let before_cancellation = runtime.block_on(async {
        let payload = serde_json::json!({"input": "cancelled-fixture-context"});
        let mut send = Box::pin(proof.post_json_stream(
            InferenceRoute::ChatCompletions,
            &payload,
            Duration::from_secs(4),
        ));
        assert!(futures_util::poll!(&mut send).is_pending());
        tokio::time::sleep(Duration::from_millis(10)).await;
        let attempted = listener.accept().map(|_| ());
        drop(send);
        attempted
    });
    release_tx.send(()).unwrap();
    runtime.block_on(blocker).unwrap();
    runtime.shutdown_timeout(Duration::from_secs(3));
    assert_eq!(
        before_cancellation.unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[cfg(target_os = "macos")]
#[test]
fn dispatch_deadline_expires_before_connect_when_native_worker_is_occupied() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let proof = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let blocker = runtime.spawn_blocking(move || {
        started_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let revision = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));
    let stale = proof.clone().bind_session(revision.clone(), 1);
    revision.store(2, std::sync::atomic::Ordering::SeqCst);
    let stale_result = runtime.block_on(stale.post_json(
        InferenceRoute::ChatCompletions,
        &serde_json::json!({"input": "stale-fixture-context"}),
        Duration::from_millis(20),
    ));
    let result = runtime.block_on(proof.post_json(
        InferenceRoute::ChatCompletions,
        &serde_json::json!({"input": "deadline-fixture-context"}),
        Duration::from_millis(20),
    ));
    release_tx.send(()).unwrap();
    runtime.block_on(blocker).unwrap();
    runtime.shutdown_timeout(Duration::from_secs(3));
    assert_eq!(
        stale_result.unwrap_err(),
        LocalInferenceError::SessionChanged
    );
    assert_eq!(result.unwrap_err(), LocalInferenceError::RequestTimedOut);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn nonstream_json_deadline_remains_effective_while_reading_response_body() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::io::{Read, Write};
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &endpoint,
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let proof = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = [0; 4096];
        assert!(socket.read(&mut bytes).unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n")
            .unwrap();
        std::thread::sleep(Duration::from_millis(750));
        let _ = socket.write_all(b"{}");
    });
    let response = proof
        .post_json(
            InferenceRoute::ChatCompletions,
            &serde_json::json!({"fixture": "context"}),
            Duration::from_millis(500),
        )
        .await
        .unwrap();
    assert_eq!(
        response.url().as_str(),
        format!("{endpoint}/v1/chat/completions")
    );
    assert!(response.text().await.is_err());
    worker.join().unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn unaccepted_channel_times_out_without_receiving_any_context_bytes() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::io::Read;
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &endpoint,
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let proof = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let error = proof
        .post_json(
            InferenceRoute::ChatCompletions,
            &serde_json::json!({"input": "must-not-enter-unverified-backlog"}),
            Duration::from_millis(300),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        LocalInferenceError::RequestTimedOut | LocalInferenceError::ListenerUnverified
    ));
    let (mut socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut bytes = [0; 4096];
    assert!(matches!(socket.read(&mut bytes), Ok(0) | Err(_)));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn native_replacement_listener_in_a_different_process_never_receives_context() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::time::{Duration, Instant};
    if let Ok(address) = std::env::var("FOUNDRYUI_REPLACEMENT_LISTENER_ADDRESS") {
        let listener = std::net::TcpListener::bind(address).unwrap();
        listener.set_nonblocking(true).unwrap();
        std::fs::write(
            std::env::var("FOUNDRYUI_REPLACEMENT_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        let release = std::env::var("FOUNDRYUI_REPLACEMENT_RELEASE").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !std::path::Path::new(&release).exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &format!("http://{address}"),
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let proof = VerifiedLocalEndpoint::verify(&binding).unwrap();
    drop(listener);
    let scratch = tempfile::tempdir().unwrap();
    let ready = scratch.path().join("ready");
    let release = scratch.path().join("release");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::native_replacement_listener_in_a_different_process_never_receives_context",
            "--nocapture",
        ])
        .env(
            "FOUNDRYUI_REPLACEMENT_LISTENER_ADDRESS",
            address.to_string(),
        )
        .env("FOUNDRYUI_REPLACEMENT_READY", &ready)
        .env("FOUNDRYUI_REPLACEMENT_RELEASE", &release)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let result = proof
        .post_json(
            InferenceRoute::ChatCompletions,
            &serde_json::json!({"input": "fixture-context-never-sent"}),
            Duration::from_secs(1),
        )
        .await;
    std::fs::write(&release, b"release").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(result.unwrap_err(), LocalInferenceError::ListenerUnverified);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn parent_epoch_binding_blocks_replay_and_stream_timeout_stops_at_headers() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::io::{Read, Write};
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &endpoint,
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let revision = Arc::new(AtomicU64::new(10));
    let proof = VerifiedLocalEndpoint::verify(&binding)
        .unwrap()
        .bind_session(revision.clone(), 10);
    revision.fetch_add(1, Ordering::SeqCst);
    let rebound = proof.clone().bind_session(revision.clone(), 11);
    assert_eq!(
        rebound.revalidate().unwrap_err(),
        LocalInferenceError::SessionChanged
    );
    assert_eq!(
        proof.revalidate().unwrap_err(),
        LocalInferenceError::SessionChanged
    );
    assert_eq!(
        proof
            .post_json(
                "/v1/chat/completions",
                &serde_json::json!({"fixture": "local context"}),
                Duration::from_secs(1)
            )
            .await
            .unwrap_err(),
        LocalInferenceError::SessionChanged
    );
    let fresh = VerifiedLocalEndpoint::verify(&binding)
        .unwrap()
        .bind_session(revision, 11);
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = [0; 4096];
        let count = socket.read(&mut bytes).unwrap();
        assert!(
            String::from_utf8_lossy(&bytes[..count])
                .starts_with("POST /v1/chat/completions HTTP/1.1\r\n")
        );
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        socket.write_all(b"{}").unwrap();
    });
    let response = fresh
        .post_json_stream(
            "/v1/chat/completions",
            &serde_json::json!({"fixture": "local context"}),
            Duration::from_millis(200),
        )
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "{}");
    worker.join().unwrap();
    assert_eq!(
        fresh
            .post_json(
                "https://example.com/stolen",
                &serde_json::json!({}),
                Duration::from_secs(1)
            )
            .await
            .unwrap_err(),
        LocalInferenceError::InvalidEndpoint
    );
    assert_eq!(
        fresh
            .post_json(
                InferenceRoute::ChatCompletions,
                &serde_json::json!({}),
                Duration::from_secs(1)
            )
            .await
            .unwrap_err(),
        LocalInferenceError::ListenerUnverified
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn isolated_process_ignores_inherited_proxy_and_proves_actual_listener_ownership() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::io::{Read, Write};
    use std::time::Duration;
    if std::env::var("FOUNDRYUI_LOCAL_INFERENCE_PROXY_CHILD").as_deref() != Ok("1") {
        let proxy_trap = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        proxy_trap.set_nonblocking(true).unwrap();
        let proxy_url = format!("http://{}", proxy_trap.local_addr().unwrap());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::isolated_process_ignores_inherited_proxy_and_proves_actual_listener_ownership", "--nocapture"])
            .env("FOUNDRYUI_LOCAL_INFERENCE_PROXY_CHILD", "1")
            .env("FOUNDRYUI_LOCAL_INFERENCE_PROXY_URL", &proxy_url)
            .env("HTTP_PROXY", &proxy_url)
            .env("HTTPS_PROXY", &proxy_url)
            .env("ALL_PROXY", &proxy_url)
            .env("http_proxy", &proxy_url)
            .env("https_proxy", &proxy_url)
            .env("all_proxy", &proxy_url)
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            proxy_trap.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        return;
    }
    let executable = std::env::current_exe().unwrap();
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &std::env::var("FOUNDRYUI_LOCAL_INFERENCE_PROXY_URL").unwrap(),
            std::process::id(),
            &executable,
            1,
        )
        .unwrap(),
    ));
    assert!(matches!(
        VerifiedLocalEndpoint::verify(&binding),
        Err(LocalInferenceError::ListenerUnverified)
    ));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    binding.replace(Some(
        RuntimeObservation::new(&endpoint, std::process::id(), &executable, 2).unwrap(),
    ));
    let verified = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let worker = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                other => panic!("fixture listener did not receive the verified POST: {other:?}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut block = [0; 4096];
        while !String::from_utf8_lossy(&bytes).contains("fixture-embedding-context") {
            let count = socket.read(&mut block).unwrap();
            assert_ne!(count, 0);
            bytes.extend_from_slice(&block[..count]);
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
        bytes
    });
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        verified.post_json(
            InferenceRoute::Embeddings,
            &serde_json::json!({"input": "fixture-embedding-context"}),
            Duration::from_secs(1),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "{}");
    let captured = String::from_utf8(worker.join().unwrap()).unwrap();
    assert!(captured.starts_with("POST /v1/embeddings HTTP/1.1\r\n"));
}

#[cfg(target_os = "macos")]
#[test]
fn binding_aba_stop_and_unknown_origin_cannot_reuse_prepared_context_proof() {
    use super::VerifiedLocalEndpoint;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let binding = RuntimeBinding::default();
    assert!(matches!(
        VerifiedLocalEndpoint::verify(&binding),
        Err(LocalInferenceError::RuntimeUnverified)
    ));
    let original = RuntimeObservation::new(
        &endpoint,
        std::process::id(),
        std::env::current_exe().unwrap(),
        4,
    )
    .unwrap();
    binding.replace(Some(original.clone()));
    let proof = VerifiedLocalEndpoint::verify(&binding).unwrap();
    binding.replace(Some(original.clone()));
    proof.revalidate().unwrap();
    binding.replace(None);
    assert_eq!(
        proof.revalidate().unwrap_err(),
        LocalInferenceError::SessionChanged
    );
    binding.replace(Some(original));
    assert_eq!(
        proof.revalidate().unwrap_err(),
        LocalInferenceError::SessionChanged
    );
    let fresh = VerifiedLocalEndpoint::verify(&binding).unwrap();
    drop(listener);
    assert_eq!(
        fresh.revalidate().unwrap_err(),
        LocalInferenceError::ListenerUnverified
    );
}

#[cfg(target_os = "macos")]
#[test]
fn resolves_runtime_from_real_cli_directory_in_a_homebrew_shaped_temp_installation() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    let libexec = root.path().join("libexec");
    std::fs::create_dir(&bin).unwrap();
    std::fs::create_dir(&libexec).unwrap();
    let cli = libexec.join("foundry");
    std::fs::write(&cli, b"fixture executable layout, never launched").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    symlink("../libexec/foundry", bin.join("foundry")).unwrap();
    assert_eq!(
        super::foundry_runtime_path(bin.join("foundry")).unwrap(),
        cli.canonicalize().unwrap()
    );

    let daemon = libexec.join("foundrylocald");
    std::fs::copy(&cli, &daemon).unwrap();
    assert_eq!(
        super::foundry_runtime_path(bin.join("foundry")).unwrap(),
        daemon.canonicalize().unwrap()
    );

    std::fs::remove_file(&daemon).unwrap();
    symlink("/usr/bin/true", &daemon).unwrap();
    assert_eq!(
        super::foundry_runtime_path(bin.join("foundry")).unwrap_err(),
        LocalInferenceError::RuntimeUnverified
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn multipart_dispatch_blocks_redirects_without_leaking_fixture_audio() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::io::{Read, Write};
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_trap = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    redirect_trap.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &endpoint,
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let verified = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let trap = redirect_trap.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut captured = Vec::new();
        let mut block = [0; 4096];
        loop {
            let count = socket.read(&mut block).unwrap();
            assert_ne!(count, 0);
            captured.extend_from_slice(&block[..count]);
            if String::from_utf8_lossy(&captured).contains("fixture-audio-bytes") {
                break;
            }
        }
        write!(socket, "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{trap}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        captured
    });
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"fixture-audio-bytes".to_vec()).file_name("fixture.wav"),
    );
    let error = verified
        .post_multipart(
            InferenceRoute::AudioTranscriptions,
            form,
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
    assert_eq!(error, LocalInferenceError::RedirectBlocked);
    let captured = String::from_utf8(worker.join().unwrap()).unwrap();
    assert!(captured.starts_with("POST /v1/audio/transcriptions HTTP/1.1\r\n"));
    assert!(captured.contains("fixture-audio-bytes"));
    assert_eq!(
        redirect_trap.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[cfg(target_os = "macos")]
#[test]
fn native_controlled_listener_proves_its_executable_not_a_name_or_label() {
    use super::VerifiedLocalEndpoint;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let observation = RuntimeObservation::new(
        &endpoint,
        std::process::id(),
        std::env::current_exe().unwrap(),
        1,
    )
    .unwrap();
    let binding = RuntimeBinding::default();
    binding.replace(Some(observation));
    let verified = VerifiedLocalEndpoint::verify(&binding).unwrap();
    verified.revalidate().unwrap();

    let impostor =
        RuntimeObservation::new(&endpoint, std::process::id(), "/usr/bin/true", 1).unwrap();
    binding.replace(Some(impostor));
    assert!(matches!(
        VerifiedLocalEndpoint::verify(&binding),
        Err(LocalInferenceError::RuntimeUnverified)
    ));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn context_send_rejects_synthetic_discovery_changes_before_contacting_listener() {
    use super::{InferenceRoute, VerifiedLocalEndpoint};
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let executable = std::env::current_exe().unwrap();
    let original = RuntimeObservation::new(&endpoint, std::process::id(), &executable, 7).unwrap();
    let binding = RuntimeBinding::default();
    binding.replace(Some(original));
    let verified = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let replacement_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    replacement_listener.set_nonblocking(true).unwrap();
    let changed_endpoint = format!("http://{}", replacement_listener.local_addr().unwrap());
    for changed in [
        RuntimeObservation::new(&changed_endpoint, std::process::id(), &executable, 7).unwrap(),
        RuntimeObservation::new(&endpoint, std::process::id(), &executable, 8).unwrap(),
        RuntimeObservation::new(&endpoint, 1, &executable, 7).unwrap(),
        RuntimeObservation::new(&endpoint, std::process::id(), "/usr/bin/true", 7).unwrap(),
    ] {
        binding.replace(Some(changed));
        assert_eq!(
            verified
                .post_json(
                    InferenceRoute::ChatCompletions,
                    &serde_json::json!({"messages": [{"content": "fixture local context"}]}),
                    Duration::from_secs(1),
                )
                .await
                .unwrap_err(),
            LocalInferenceError::SessionChanged
        );
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        replacement_listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn rejects_ambiguous_remote_or_decorated_endpoints_without_echoing_input() {
    for endpoint in [
        "https://example.com:443",
        "http://localhost:8000",
        "http://127.1:8000",
        "http://2130706433:8000",
        "http://0x7f000001:8000",
        "http://127.0.0.1",
        "http://127.0.0.1:0",
        "http://127.0.0.1:65536",
        "http://user:secret@127.0.0.1:8000",
        "http://127.0.0.1:8000/?secret=contents",
        "http://127.0.0.1:8000/#contents",
        "http://127.0.0.1:8000/v1",
        "http://127.0.0.1:8000/../",
        "http://127.0.0.1:8000/ ",
        " http://127.0.0.1:8000",
        "http://[::ffff:127.0.0.1]:8000",
        "http://0.0.0.0:8000",
    ] {
        let error = RuntimeObservation::new(endpoint, 1, "/usr/bin/true", 1)
            .expect_err("unverified destinations cannot become backend observations");
        assert_eq!(error, LocalInferenceError::InvalidEndpoint);
        assert!(!format!("{error:?} {error}").contains(endpoint));
    }
}
