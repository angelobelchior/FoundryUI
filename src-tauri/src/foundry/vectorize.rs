use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Default)]
pub struct VectorizeRegistry {
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl VectorizeRegistry {
    pub fn begin(&self, request_id: &str) -> Arc<AtomicBool> {
        let cancellation = Arc::new(AtomicBool::new(false));

        lock(&self.active).insert(request_id.to_string(), cancellation.clone());

        cancellation
    }

    pub fn cancel(&self, request_id: &str) -> bool {
        match lock(&self.active).get(request_id) {
            Some(token) => {
                token.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn finish(&self, request_id: &str) {
        lock(&self.active).remove(request_id);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_marks_the_request() {
        let registry = VectorizeRegistry::default();
        let token = registry.begin("req-1");

        assert!(registry.cancel("req-1"));
        assert!(token.load(Ordering::SeqCst));
        assert!(!registry.cancel("missing"));
    }

    #[test]
    fn finish_removes_the_request() {
        let registry = VectorizeRegistry::default();

        registry.begin("req-1");
        registry.finish("req-1");

        assert!(!registry.cancel("req-1"));
    }
}
