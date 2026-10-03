use rcdesktop::wslc::mock::MockWslcClient;
use rcdesktop::wslc::client::WslcClient;

#[tokio::test]
async fn test_mock_client_containers_crud() {
    let client = MockWslcClient::new();
    let initial = client.list_containers(true).await.expect("List failed");
    assert!(!initial.is_empty());

    let target_id = initial[0].id.clone();
    client.stop_container(&target_id).await.expect("Stop failed");

    let updated = client.list_containers(true).await.expect("List failed");
    let stopped = updated.iter().find(|c| c.id == target_id).unwrap();
    assert_eq!(stopped.state, rcdesktop::domain::container::ContainerState::Exited(0));

    client.start_container(&target_id).await.expect("Start failed");
    let running_list = client.list_containers(true).await.expect("List failed");
    let started = running_list.iter().find(|c| c.id == target_id).unwrap();
    assert_eq!(started.state, rcdesktop::domain::container::ContainerState::Running);
}

#[tokio::test]
async fn test_mock_client_images_pull_and_remove() {
    let client = MockWslcClient::new();
    client.pull_image("redis:7-alpine").await.expect("Pull failed");

    let images = client.list_images().await.expect("Images failed");
    assert!(images.iter().any(|i| i.repository == "redis" && i.tag == "7-alpine"));

    let target = images.iter().find(|i| i.repository == "redis").unwrap();
    let target_id = target.id.clone();
    client.remove_image(&target_id).await.expect("Remove failed");

    let after_removal = client.list_images().await.expect("Images failed");
    assert!(!after_removal.iter().any(|i| i.id == target_id));
}
