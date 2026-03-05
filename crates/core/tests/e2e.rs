use std::time::Duration;

use rqs_lib::channel::{ChannelAction, ChannelDirection, ChannelMessage, TransferType};
use rqs_lib::{DeviceType, EndpointInfo, OutboundPayload, SendInfo, State, Visibility, RQS};
use tokio::sync::broadcast;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Helper: collect channel messages until a predicate is satisfied.
async fn wait_for_state(
    rx: &mut broadcast::Receiver<ChannelMessage>,
    target_state: State,
) -> ChannelMessage {
    timeout(TEST_TIMEOUT, async {
        loop {
            match rx.recv().await {
                Ok(msg) if msg.state == Some(target_state.clone()) => return msg,
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("channel lagged by {n} messages");
                    continue;
                }
                Err(e) => panic!("channel error: {e}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for state {:?}", target_state))
}

/// Helper: wait for a specific state on a specific connection id.
async fn wait_for_state_with_id(
    rx: &mut broadcast::Receiver<ChannelMessage>,
    target_state: State,
    id: &str,
) -> ChannelMessage {
    timeout(TEST_TIMEOUT, async {
        loop {
            match rx.recv().await {
                Ok(msg) if msg.state == Some(target_state.clone()) && msg.id == id => return msg,
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("channel lagged by {n} messages");
                    continue;
                }
                Err(e) => panic!("channel error: {e}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for state {:?} on id {}", target_state, id))
}

/// Helper: collect all messages matching a given direction until a state is reached.
async fn collect_states_until(
    rx: &mut broadcast::Receiver<ChannelMessage>,
    direction: ChannelDirection,
    terminal_state: State,
    id: &str,
) -> Vec<ChannelMessage> {
    let mut collected = Vec::new();
    timeout(TEST_TIMEOUT, async {
        loop {
            match rx.recv().await {
                Ok(msg) if msg.direction == direction && msg.id == id => {
                    let is_terminal = msg.state == Some(terminal_state.clone());
                    collected.push(msg);
                    if is_terminal {
                        return;
                    }
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("channel lagged by {n} messages");
                    continue;
                }
                Err(e) => panic!("channel error: {e}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timeout collecting states until {:?} on id {}",
            terminal_state, id
        )
    });
    collected
}

/// Start a receiver RQS instance, returning (RQS, send_channel, port).
async fn start_receiver(
    download_dir: std::path::PathBuf,
) -> (RQS, tokio::sync::mpsc::Sender<SendInfo>, u16) {
    let mut rqs = RQS::new(Visibility::Visible, None, Some(download_dir));
    let (send_ch, _) = rqs.run().await.expect("receiver should start");
    let port = rqs.bound_addr.expect("bound_addr should be set").port();
    (rqs, send_ch, port)
}

/// Start a sender RQS instance, returning (RQS, send_channel).
async fn start_sender() -> (RQS, tokio::sync::mpsc::Sender<SendInfo>) {
    let mut rqs = RQS::new(Visibility::Visible, None, None);
    let (send_ch, _) = rqs.run().await.expect("sender should start");
    (rqs, send_ch)
}

/// Helper: accept a transfer on the receiver side.
fn accept_transfer(receiver: &RQS, connection_id: &str) {
    receiver
        .message_sender
        .send(ChannelMessage {
            id: connection_id.to_string(),
            direction: ChannelDirection::FrontToLib,
            action: Some(ChannelAction::AcceptTransfer),
            ..Default::default()
        })
        .unwrap();
}

/// Helper: reject a transfer on the receiver side.
fn reject_transfer(receiver: &RQS, connection_id: &str) {
    receiver
        .message_sender
        .send(ChannelMessage {
            id: connection_id.to_string(),
            direction: ChannelDirection::FrontToLib,
            action: Some(ChannelAction::RejectTransfer),
            ..Default::default()
        })
        .unwrap();
}

/// Helper: initiate a file send from sender to receiver.
async fn initiate_send(
    sender_send: &tokio::sync::mpsc::Sender<SendInfo>,
    receiver_addr: &str,
    file_paths: Vec<String>,
) {
    sender_send
        .send(SendInfo {
            id: receiver_addr.to_string(),
            name: "E2E Test".to_string(),
            addr: receiver_addr.to_string(),
            ob: OutboundPayload::Files(file_paths),
        })
        .await
        .unwrap();
}

// ─── File Transfer Tests ───────────────────────────────────────────────────

#[tokio::test]
async fn test_single_file_transfer() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let test_content = b"Hello from quickshare E2E test! This is a real file transfer.";
    let test_file = source_dir.path().join("test_file.txt");
    std::fs::write(&test_file, test_content).unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut sender_msgs = sender.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    let connection_id = consent_msg.id.clone();

    // Verify metadata
    let meta = consent_msg.meta.as_ref().unwrap();
    assert!(meta.files.is_some(), "should list files");
    assert!(meta.pin_code.is_some(), "should have pin code");
    assert_eq!(meta.total_bytes, test_content.len() as u64);
    assert_eq!(consent_msg.direction, ChannelDirection::LibToFront);
    assert_eq!(consent_msg.rtype, Some(TransferType::Inbound));

    accept_transfer(&receiver, &connection_id);

    wait_for_state_with_id(&mut recv_msgs, State::Finished, &connection_id).await;
    wait_for_state(&mut sender_msgs, State::Finished).await;

    // Verify the file was received with correct content
    let received_file = download_dir.path().join("test_file.txt");
    assert!(received_file.exists(), "received file should exist");
    let received_content = std::fs::read(&received_file).unwrap();
    assert_eq!(received_content, test_content, "file content should match");

    sender.stop().await;
    receiver.stop().await;
}

#[tokio::test]
async fn test_multiple_files_transfer() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let files_data: Vec<(&str, Vec<u8>)> = vec![
        ("file_a.txt", b"Content of file A".to_vec()),
        (
            "file_b.txt",
            b"Content of file B - slightly longer content here".to_vec(),
        ),
        ("file_c.bin", vec![0u8, 1, 2, 3, 4, 5, 255, 254, 253]),
    ];

    let mut file_paths = Vec::new();
    for (name, content) in &files_data {
        let path = source_dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        file_paths.push(path.to_string_lossy().to_string());
    }

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut sender_msgs = sender.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(&sender_send, &receiver_addr, file_paths).await;

    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    let connection_id = consent_msg.id.clone();

    // Verify metadata reports all files
    let meta = consent_msg.meta.as_ref().unwrap();
    let files_list = meta.files.as_ref().unwrap();
    assert_eq!(files_list.len(), 3, "should report 3 files");

    // Verify total_bytes is the sum of all files
    let expected_total: u64 = files_data.iter().map(|(_, c)| c.len() as u64).sum();
    assert_eq!(
        meta.total_bytes, expected_total,
        "total_bytes should be sum of all file sizes"
    );

    accept_transfer(&receiver, &connection_id);

    wait_for_state_with_id(&mut recv_msgs, State::Finished, &connection_id).await;
    wait_for_state(&mut sender_msgs, State::Finished).await;

    // Verify all files byte-for-byte
    for (name, expected_content) in &files_data {
        let received = download_dir.path().join(name);
        assert!(received.exists(), "file {} should exist", name);
        let content = std::fs::read(&received).unwrap();
        assert_eq!(
            content, *expected_content,
            "content of {} should match",
            name
        );
    }

    sender.stop().await;
    receiver.stop().await;
}

#[tokio::test]
async fn test_large_file_transfer() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    // 2MB file with a known pattern
    let size = 2 * 1024 * 1024;
    let test_content: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    let test_file = source_dir.path().join("large_file.bin");
    std::fs::write(&test_file, &test_content).unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut sender_msgs = sender.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    let connection_id = consent_msg.id.clone();

    let meta = consent_msg.meta.as_ref().unwrap();
    assert_eq!(
        meta.total_bytes, size as u64,
        "total bytes should match file size"
    );

    accept_transfer(&receiver, &connection_id);

    // Collect all receiver messages to verify we see ReceivingFiles state
    let recv_states = collect_states_until(
        &mut recv_msgs,
        ChannelDirection::LibToFront,
        State::Finished,
        &connection_id,
    )
    .await;
    wait_for_state(&mut sender_msgs, State::Finished).await;

    // Verify we went through ReceivingFiles state
    assert!(
        recv_states
            .iter()
            .any(|m| m.state == Some(State::ReceivingFiles)),
        "should pass through ReceivingFiles state for large file"
    );

    // Verify ack_bytes in the Finished message
    let finished_msg = recv_states
        .iter()
        .find(|m| m.state == Some(State::Finished))
        .unwrap();
    if let Some(meta) = &finished_msg.meta {
        assert_eq!(
            meta.ack_bytes, size as u64,
            "ack_bytes should equal file size at finish"
        );
    }

    let received_file = download_dir.path().join("large_file.bin");
    assert!(received_file.exists(), "large file should exist");
    let received_content = std::fs::read(&received_file).unwrap();
    assert_eq!(
        received_content.len(),
        test_content.len(),
        "file size should match"
    );
    assert_eq!(
        received_content, test_content,
        "file content should match byte-for-byte"
    );

    sender.stop().await;
    receiver.stop().await;
}

// NOTE: 0-byte file transfer is not tested because the Nearby Share protocol
// doesn't handle empty files — the sender sends no file chunks, so the receiver
// never reaches Finished state.

#[tokio::test]
async fn test_binary_file_integrity() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    // File with every possible byte value to catch encoding issues
    let test_content: Vec<u8> = (0..=255u8).cycle().take(1024 * 100).collect(); // 100KB, all byte values
    let test_file = source_dir.path().join("all_bytes.bin");
    std::fs::write(&test_file, &test_content).unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut sender_msgs = sender.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    accept_transfer(&receiver, &consent_msg.id);

    wait_for_state_with_id(&mut recv_msgs, State::Finished, &consent_msg.id).await;
    wait_for_state(&mut sender_msgs, State::Finished).await;

    let received = std::fs::read(download_dir.path().join("all_bytes.bin")).unwrap();
    assert_eq!(received.len(), test_content.len(), "size should match");
    // Check specific byte values at boundaries
    assert_eq!(received[0], 0, "first byte");
    assert_eq!(received[255], 255, "byte 255");
    assert_eq!(received[256], 0, "wraps at 256");
    assert_eq!(received, test_content, "full content should match");

    sender.stop().await;
    receiver.stop().await;
}

// ─── Reject / Cancel Tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_reject_transfer() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let test_file = source_dir.path().join("rejected.txt");
    std::fs::write(&test_file, b"this should not be received").unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    let connection_id = consent_msg.id.clone();

    // Verify metadata is still provided even though we'll reject
    assert!(consent_msg.meta.as_ref().unwrap().pin_code.is_some());

    reject_transfer(&receiver, &connection_id);

    // Receiver should emit Rejected state
    wait_for_state_with_id(&mut recv_msgs, State::Rejected, &connection_id).await;

    // File should NOT exist in download dir
    let received = download_dir.path().join("rejected.txt");
    assert!(!received.exists(), "rejected file should not be received");

    // Verify download dir is completely empty (no partial files)
    let dir_entries: Vec<_> = std::fs::read_dir(download_dir.path()).unwrap().collect();
    assert!(
        dir_entries.is_empty(),
        "download dir should be empty after reject"
    );

    sender.stop().await;
    receiver.stop().await;
}

// ─── PIN / Security Tests ──────────────────────────────────────────────────

#[tokio::test]
async fn test_pin_codes_match() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let test_file = source_dir.path().join("pin_test.txt");
    std::fs::write(&test_file, b"pin code verification").unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut sender_msgs = sender.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    // Get pin from receiver side
    let recv_consent = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    let recv_pin = recv_consent
        .meta
        .as_ref()
        .unwrap()
        .pin_code
        .clone()
        .unwrap();

    // Get pin from sender side
    let sender_intro = wait_for_state(&mut sender_msgs, State::SentIntroduction).await;
    let sender_pin = sender_intro
        .meta
        .as_ref()
        .unwrap()
        .pin_code
        .clone()
        .unwrap();

    // PIN codes must match — this is the core security guarantee
    assert_eq!(
        recv_pin, sender_pin,
        "PIN codes should match between sender and receiver"
    );
    assert_eq!(recv_pin.len(), 4, "PIN should be 4 digits");
    assert!(
        recv_pin.chars().all(|c| c.is_ascii_digit()),
        "PIN should contain only digits, got: {}",
        recv_pin
    );

    // Accept and finish cleanly
    accept_transfer(&receiver, &recv_consent.id);
    wait_for_state_with_id(&mut recv_msgs, State::Finished, &recv_consent.id).await;

    sender.stop().await;
    receiver.stop().await;
}

// ─── Metadata Tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_transfer_metadata_correctness() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let test_file = source_dir.path().join("metadata_test.txt");
    let content = b"metadata verification content";
    std::fs::write(&test_file, content).unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    let meta = consent_msg.meta.as_ref().unwrap();

    // Verify all metadata fields
    assert_eq!(meta.total_bytes, content.len() as u64);
    assert_eq!(consent_msg.direction, ChannelDirection::LibToFront);
    assert_eq!(consent_msg.rtype, Some(TransferType::Inbound));

    // Source device info
    let source = meta
        .source
        .as_ref()
        .expect("source device info should be present");
    assert!(!source.name.is_empty(), "source name should not be empty");
    assert_eq!(
        source.device_type,
        DeviceType::Laptop,
        "source should be laptop"
    );

    // PIN code format
    let pin = meta.pin_code.as_ref().expect("pin code should be present");
    assert_eq!(pin.len(), 4, "PIN should be 4 chars");
    assert!(
        pin.chars().all(|c| c.is_ascii_digit()),
        "PIN should be numeric"
    );

    // Files list
    let files = meta.files.as_ref().unwrap();
    assert_eq!(files.len(), 1, "should report 1 file");
    assert!(
        files[0].contains("metadata_test.txt"),
        "file name should be in metadata, got: {}",
        files[0]
    );

    // Clean up by rejecting
    reject_transfer(&receiver, &consent_msg.id);

    sender.stop().await;
    receiver.stop().await;
}

#[tokio::test]
async fn test_sender_state_machine_progression() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let test_file = source_dir.path().join("states.txt");
    std::fs::write(&test_file, b"state machine test").unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut sender_msgs = sender.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    // Accept on receiver side
    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    accept_transfer(&receiver, &consent_msg.id);

    // Collect all sender messages through Finished.
    // Note: sender's connection id differs from receiver's (sender uses receiver_addr,
    // receiver uses sender's ephemeral addr), so we get the sender id from its first message.
    let sender_intro = wait_for_state(&mut sender_msgs, State::SentIntroduction).await;
    let sender_id = sender_intro.id.clone();
    let mut sender_states = vec![sender_intro];

    let rest = collect_states_until(
        &mut sender_msgs,
        ChannelDirection::LibToFront,
        State::Finished,
        &sender_id,
    )
    .await;
    sender_states.extend(rest);

    let state_seq: Vec<State> = sender_states
        .iter()
        .filter_map(|m| m.state.clone())
        .collect();

    // Verify all sender states are Outbound type
    for msg in &sender_states {
        assert_eq!(
            msg.rtype,
            Some(TransferType::Outbound),
            "all sender states should be Outbound, got {:?} at state {:?}",
            msg.rtype,
            msg.state
        );
    }

    // Verify key states appear in order
    let must_appear = [
        State::SentIntroduction,
        State::SendingFiles,
        State::Finished,
    ];
    for required in &must_appear {
        assert!(
            state_seq.contains(required),
            "sender should pass through {:?}, states were: {:?}",
            required,
            state_seq
        );
    }

    // Verify SentIntroduction comes before SendingFiles
    let intro_pos = state_seq.iter().position(|s| *s == State::SentIntroduction);
    let sending_pos = state_seq.iter().position(|s| *s == State::SendingFiles);
    assert!(
        intro_pos < sending_pos,
        "SentIntroduction should come before SendingFiles"
    );

    sender.stop().await;
    receiver.stop().await;
}

#[tokio::test]
async fn test_receiver_state_machine_progression() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let test_file = source_dir.path().join("recv_states.txt");
    std::fs::write(
        &test_file,
        b"receiver state machine test content for tracking",
    )
    .unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut recv_msgs2 = receiver.message_sender.subscribe();
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    initiate_send(
        &sender_send,
        &receiver_addr,
        vec![test_file.to_string_lossy().to_string()],
    )
    .await;

    // Wait for consent then accept
    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    let connection_id = consent_msg.id.clone();
    accept_transfer(&receiver, &connection_id);

    // Collect all receiver messages through Finished
    let recv_states = collect_states_until(
        &mut recv_msgs2,
        ChannelDirection::LibToFront,
        State::Finished,
        &connection_id,
    )
    .await;

    let state_seq: Vec<State> = recv_states.iter().filter_map(|m| m.state.clone()).collect();

    // Verify all receiver states are Inbound type
    for msg in &recv_states {
        assert_eq!(
            msg.rtype,
            Some(TransferType::Inbound),
            "all receiver states should be Inbound, got {:?} at state {:?}",
            msg.rtype,
            msg.state
        );
    }

    // Verify key states appear
    assert!(
        state_seq.contains(&State::WaitingForUserConsent),
        "should have WaitingForUserConsent"
    );
    assert!(state_seq.contains(&State::Finished), "should have Finished");

    // Verify WaitingForUserConsent comes before Finished
    let consent_pos = state_seq
        .iter()
        .position(|s| *s == State::WaitingForUserConsent);
    let finished_pos = state_seq.iter().position(|s| *s == State::Finished);
    assert!(
        consent_pos < finished_pos,
        "WaitingForUserConsent should come before Finished"
    );

    // Verify destination is reported in finished message
    let finished_msg = recv_states
        .iter()
        .find(|m| m.state == Some(State::Finished))
        .unwrap();
    if let Some(meta) = &finished_msg.meta {
        assert!(
            meta.destination.is_some(),
            "finished message should include download destination"
        );
    }

    sender.stop().await;
    receiver.stop().await;
}

// ─── mDNS Discovery Tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_mdns_discovery() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();

    let (mut advertiser, _adv_send, advertiser_port) =
        start_receiver(download_dir.path().to_path_buf()).await;

    let mut discoverer = RQS::new(Visibility::Visible, None, None);
    let (_disc_send, _) = discoverer.run().await.expect("discoverer should start");

    let (discovery_tx, mut discovery_rx) = broadcast::channel::<EndpointInfo>(10);
    discoverer
        .discovery(discovery_tx)
        .expect("discovery should start");

    let discovered = timeout(Duration::from_secs(15), async {
        loop {
            match discovery_rx.recv().await {
                Ok(ei) => {
                    if ei.present == Some(true) && ei.port == Some(advertiser_port.to_string()) {
                        return ei;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("discovery channel lagged by {n}");
                    continue;
                }
                Err(e) => panic!("discovery channel error: {e}"),
            }
        }
    })
    .await
    .expect("should discover the advertiser within 15s");

    // Strict endpoint info verification
    assert!(discovered.name.is_some(), "should have a name");
    assert!(
        !discovered.name.as_ref().unwrap().is_empty(),
        "name should not be empty"
    );
    assert!(discovered.ip.is_some(), "should have an IP");
    assert_eq!(discovered.port, Some(advertiser_port.to_string()));
    assert_eq!(
        discovered.rtype,
        Some(DeviceType::Laptop),
        "device type should be Laptop"
    );
    assert_eq!(discovered.present, Some(true));

    // The id should be "ip:port" format
    let id_parts: Vec<&str> = discovered.id.split(':').collect();
    assert_eq!(id_parts.len(), 2, "id should be ip:port format");
    assert_eq!(
        id_parts[1],
        advertiser_port.to_string(),
        "port in id should match"
    );

    // IP in the id should match the ip field
    assert_eq!(
        id_parts[0],
        discovered.ip.as_ref().unwrap(),
        "IP in id should match ip field"
    );

    // Fullname should follow mDNS format
    assert!(
        discovered.fullname.ends_with("._FC9F5ED42C8A._tcp.local."),
        "fullname should end with service type, got: {}",
        discovered.fullname
    );

    discoverer.stop().await;
    advertiser.stop().await;
}

#[tokio::test]
async fn test_discovery_then_transfer() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let test_content = b"Discovered and transferred!";
    let test_file = source_dir.path().join("discovered.txt");
    std::fs::write(&test_file, test_content).unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let (mut sender, sender_send) = start_sender().await;
    receiver.set_download_path(Some(download_dir.path().to_path_buf()));

    let (discovery_tx, mut discovery_rx) = broadcast::channel::<EndpointInfo>(10);
    sender
        .discovery(discovery_tx)
        .expect("discovery should start");

    // Discover the receiver
    let discovered = timeout(Duration::from_secs(15), async {
        loop {
            match discovery_rx.recv().await {
                Ok(ei)
                    if ei.present == Some(true) && ei.port == Some(receiver_port.to_string()) =>
                {
                    return ei;
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("discovery channel error: {e}"),
            }
        }
    })
    .await
    .expect("should discover receiver");

    sender.stop_discovery();

    let mut recv_msgs = receiver.message_sender.subscribe();
    let mut sender_msgs = sender.message_sender.subscribe();

    // Use the discovered endpoint info to send
    sender_send
        .send(SendInfo {
            id: discovered.id.clone(),
            name: discovered.name.clone().unwrap_or_default(),
            addr: discovered.id.clone(),
            ob: OutboundPayload::Files(vec![test_file.to_string_lossy().to_string()]),
        })
        .await
        .unwrap();

    let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
    accept_transfer(&receiver, &consent_msg.id);

    wait_for_state_with_id(&mut recv_msgs, State::Finished, &consent_msg.id).await;
    wait_for_state(&mut sender_msgs, State::Finished).await;

    let received_file = download_dir.path().join("discovered.txt");
    assert!(received_file.exists(), "file should be received");
    assert_eq!(std::fs::read(&received_file).unwrap(), test_content);

    sender.stop().await;
    receiver.stop().await;
}

#[tokio::test]
async fn test_discovery_stop_clears_browse() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();

    let (mut advertiser, _adv_send, _advertiser_port) =
        start_receiver(download_dir.path().to_path_buf()).await;

    let mut discoverer = RQS::new(Visibility::Visible, None, None);
    let (_disc_send, _) = discoverer.run().await.expect("discoverer should start");

    // Start and immediately stop discovery multiple times without panicking
    for _ in 0..3 {
        let (tx, _rx) = broadcast::channel::<EndpointInfo>(10);
        discoverer.discovery(tx).expect("discovery should start");
        tokio::time::sleep(Duration::from_millis(100)).await;
        discoverer.stop_discovery();
    }

    discoverer.stop().await;
    advertiser.stop().await;
}

/// Reproduces the "Failed to send SearchStarted: sending on a closed channel" error.
/// The bug occurs when discovery finds a device, is cancelled, and the daemon's
/// periodic browse timer fires after the receiver channel is dropped.
#[tokio::test]
async fn test_discovery_cancel_no_channel_error() {
    // Enable mdns_sd logs so the test fails visibly if the fix regresses
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=warn"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();

    // Start an advertiser so there's a real service to discover
    let (mut advertiser, _adv_send, advertiser_port) =
        start_receiver(download_dir.path().to_path_buf()).await;

    let mut discoverer = RQS::new(Visibility::Visible, None, None);
    let (_disc_send, _) = discoverer.run().await.expect("discoverer should start");

    // Start discovery and wait until we find the advertiser
    let (discovery_tx, mut discovery_rx) = broadcast::channel::<EndpointInfo>(10);
    discoverer
        .discovery(discovery_tx)
        .expect("discovery should start");

    timeout(Duration::from_secs(15), async {
        loop {
            match discovery_rx.recv().await {
                Ok(ei)
                    if ei.present == Some(true) && ei.port == Some(advertiser_port.to_string()) =>
                {
                    return;
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("discovery channel error: {e}"),
            }
        }
    })
    .await
    .expect("should discover the advertiser");

    // Cancel discovery — this is where the bug would trigger:
    // the daemon's periodic SearchStarted timer would fire after
    // the browse receiver is dropped.
    discoverer.stop_discovery();

    // Wait long enough for the daemon's periodic timer to fire.
    // The daemon resends SearchStarted every few seconds.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // If we get here without mdns_sd logging errors, the fix works.
    discoverer.stop().await;
    advertiser.stop().await;
}

// ─── Visibility Tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_invisible_device_not_discoverable() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    // Start an INVISIBLE device
    let download_dir = tempfile::tempdir().unwrap();
    let mut invisible = RQS::new(
        Visibility::Invisible,
        None,
        Some(download_dir.path().to_path_buf()),
    );
    let (_send, _) = invisible.run().await.expect("invisible should start");
    let invisible_port = invisible.bound_addr.unwrap().port();

    // Start a discoverer
    let mut discoverer = RQS::new(Visibility::Visible, None, None);
    let (_disc_send, _) = discoverer.run().await.expect("discoverer should start");

    let (discovery_tx, mut discovery_rx) = broadcast::channel::<EndpointInfo>(10);
    discoverer
        .discovery(discovery_tx)
        .expect("discovery should start");

    // Wait a few seconds — the invisible device should NOT be discovered
    let result = timeout(Duration::from_secs(5), async {
        loop {
            match discovery_rx.recv().await {
                Ok(ei)
                    if ei.present == Some(true) && ei.port == Some(invisible_port.to_string()) =>
                {
                    return ei;
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => panic!("channel closed"),
            }
        }
    })
    .await;

    assert!(
        result.is_err(),
        "invisible device should NOT be discoverable (timed out = correct)"
    );

    discoverer.stop().await;
    invisible.stop().await;
}

#[tokio::test]
async fn test_visibility_change_affects_discovery() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();

    // Start as invisible
    let mut device = RQS::new(
        Visibility::Invisible,
        None,
        Some(download_dir.path().to_path_buf()),
    );
    let (_send, _) = device.run().await.expect("device should start");
    let device_port = device.bound_addr.unwrap().port();

    let mut discoverer = RQS::new(Visibility::Visible, None, None);
    let (_disc_send, _) = discoverer.run().await.expect("discoverer should start");

    let (discovery_tx, mut discovery_rx) = broadcast::channel::<EndpointInfo>(10);
    discoverer
        .discovery(discovery_tx)
        .expect("discovery should start");

    // Should NOT be discovered while invisible
    let not_found = timeout(Duration::from_secs(3), async {
        loop {
            match discovery_rx.recv().await {
                Ok(ei) if ei.present == Some(true) && ei.port == Some(device_port.to_string()) => {
                    return ei;
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => panic!("closed"),
            }
        }
    })
    .await;
    assert!(not_found.is_err(), "should not be found while invisible");

    // Now change to visible
    device.change_visibility(Visibility::Visible);

    // Should now be discoverable
    let found = timeout(Duration::from_secs(15), async {
        loop {
            match discovery_rx.recv().await {
                Ok(ei) if ei.present == Some(true) && ei.port == Some(device_port.to_string()) => {
                    return ei;
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => panic!("closed"),
            }
        }
    })
    .await;
    assert!(
        found.is_ok(),
        "should be discoverable after changing to Visible"
    );

    discoverer.stop().await;
    device.stop().await;
}

// ─── Concurrent / Stress Tests ─────────────────────────────────────────────

#[tokio::test]
async fn test_back_to_back_transfers() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,mdns_sd=off"),
    )
    .is_test(true)
    .try_init();

    let download_dir = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();

    let (mut receiver, _recv_send, receiver_port) =
        start_receiver(download_dir.path().to_path_buf()).await;
    let receiver_addr = format!("127.0.0.1:{}", receiver_port);

    // Perform 3 sequential transfers on the same receiver
    for i in 0..3 {
        let filename = format!("transfer_{i}.txt");
        let content = format!("Content for transfer number {i}");
        let test_file = source_dir.path().join(&filename);
        std::fs::write(&test_file, content.as_bytes()).unwrap();

        let (mut sender, sender_send) = start_sender().await;
        receiver.set_download_path(Some(download_dir.path().to_path_buf()));

        let mut recv_msgs = receiver.message_sender.subscribe();
        let mut sender_msgs = sender.message_sender.subscribe();

        initiate_send(
            &sender_send,
            &receiver_addr,
            vec![test_file.to_string_lossy().to_string()],
        )
        .await;

        let consent_msg = wait_for_state(&mut recv_msgs, State::WaitingForUserConsent).await;
        accept_transfer(&receiver, &consent_msg.id);

        wait_for_state_with_id(&mut recv_msgs, State::Finished, &consent_msg.id).await;
        wait_for_state(&mut sender_msgs, State::Finished).await;

        let received = download_dir.path().join(&filename);
        assert!(received.exists(), "transfer {i}: file should exist");
        assert_eq!(
            std::fs::read_to_string(&received).unwrap(),
            content,
            "transfer {i}: content should match"
        );

        sender.stop().await;
    }

    receiver.stop().await;
}
