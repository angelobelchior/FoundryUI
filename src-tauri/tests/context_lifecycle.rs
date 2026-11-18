use std::sync::Arc;

use foundryui_lib::foundry::chat::GenerationRegistry;

#[test]
fn lifecycle_changes_wait_until_the_live_generation_has_finished() {
    let registry = Arc::new(GenerationRegistry::default());
    let generation = registry.begin(41).expect("generation");
    assert_eq!(registry.with_idle_conversations(&[41], || "changed"), None);
    assert_eq!(
        registry.with_idle_conversations(&[42], || "changed"),
        Some("changed")
    );
    assert!(registry.cancel(&generation.request_id));
    assert_eq!(registry.with_idle_conversations(&[41], || "changed"), None);
    drop(generation);
    assert_eq!(
        registry.with_idle_conversations(&[41], || "changed"),
        Some("changed")
    );
}
