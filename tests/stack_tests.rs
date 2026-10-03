use rcdesktop::app::AppController;
use rcdesktop::ContainerItem;
use slint::Model;

fn item(id: &str, state: &str, stack: &str) -> ContainerItem {
    ContainerItem {
        id: id.into(),
        name: id.into(),
        state: state.into(),
        stack: stack.into(),
        ..Default::default()
    }
}

#[test]
fn test_group_by_stack() {
    let (stacks, standalone) = AppController::group_by_stack(vec![
        item("web", "Running", "shop"),
        item("solo", "Exited", ""),
        item("db", "Exited", "shop"),
        item("worker", "Running", "analytics"),
    ]);

    assert_eq!(stacks.len(), 2);
    // Sorted by name
    assert_eq!(stacks[0].name, "analytics");
    assert_eq!(stacks[1].name, "shop");
    assert_eq!(stacks[1].total, 2);
    assert_eq!(stacks[1].running, 1);
    assert_eq!(stacks[1].containers.row_count(), 2);
    assert!(!stacks[1].collapsed);

    assert_eq!(standalone.len(), 1);
    assert_eq!(standalone[0].id, "solo");
}
