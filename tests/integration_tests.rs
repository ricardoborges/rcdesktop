use rcdesktop::domain::container::ContainerState;
use rcdesktop::wslc::client::WslcClient;
use rcdesktop::wslc::mock::MockWslcClient;

#[tokio::test]
async fn test_end_to_end_mock_workflow() {
    let client = MockWslcClient::new();

    // 1. Session health
    let session = client.get_session_info().await.expect("Session check failed");
    assert!(session.is_healthy);
    assert_eq!(session.version, "5.0.1.1");

    // 2. Container lifecycle
    let containers = client.list_containers(true).await.expect("List failed");
    assert!(!containers.is_empty());
    let initial_count = containers.len();

    let target = &containers[0];
    let target_id = target.id.clone();

    // Stop container
    client.stop_container(&target_id).await.expect("Stop failed");
    let after_stop = client.list_containers(true).await.expect("List failed");
    let stopped = after_stop.iter().find(|c| c.id == target_id).unwrap();
    assert_eq!(stopped.state, ContainerState::Exited(0));

    // Get logs
    let logs = client.get_logs(&target_id, 10).await.expect("Logs failed");
    assert!(logs.contains("Container"));

    // Inspect container
    let inspect = client.inspect_container(&target_id).await.expect("Inspect failed");
    assert!(inspect.contains(&target_id));

    // Restart container
    client.restart_container(&target_id).await.expect("Restart failed");
    let after_restart = client.list_containers(true).await.expect("List failed");
    let restarted = after_restart.iter().find(|c| c.id == target_id).unwrap();
    assert_eq!(restarted.state, ContainerState::Running);

    // 3. Image pull and delete
    client.pull_image("alpine:latest").await.expect("Pull failed");
    let images = client.list_images().await.expect("Images failed");
    assert!(images.iter().any(|i| i.repository == "alpine" && i.tag == "latest"));

    // 4. Volume list
    let volumes = client.list_volumes().await.expect("Volumes failed");
    assert!(!volumes.is_empty());

    // 5. Container removal
    client.remove_container(&target_id).await.expect("Remove container failed");
    let final_containers = client.list_containers(true).await.expect("List failed");
    assert_eq!(final_containers.len(), initial_count - 1);
}
