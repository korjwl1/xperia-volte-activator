//! Engine event delivery. Desktop and CLI share every operation; only the destination differs.
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use tauri::Emitter;

type Callback = dyn Fn(&str, Value) -> Result<(), String> + Send + Sync;

#[derive(Clone)]
pub(crate) struct Events(Arc<Callback>);

impl Events {
    pub fn desktop(app: tauri::AppHandle) -> Self {
        Self(Arc::new(move |name, payload| {
            app.emit(name, payload).map_err(|e| e.to_string())
        }))
    }

    #[cfg(any(feature = "dev-cli", test))]
    pub fn callback(f: impl Fn(&str, Value) -> Result<(), String> + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    pub fn emit(&self, name: &str, payload: impl Serialize) -> Result<(), String> {
        (self.0)(
            name,
            serde_json::to_value(payload).map_err(|e| e.to_string())?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_shape_and_clones_reach_the_same_destination() {
        let received = Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = received.clone();
        let events = Events::callback(move |name, value| {
            output.lock().unwrap().push((name.to_owned(), value));
            Ok(())
        });
        events
            .clone()
            .emit("efs:progress", serde_json::json!({"n": 2}))
            .unwrap();
        assert_eq!(
            *received.lock().unwrap(),
            vec![("efs:progress".into(), serde_json::json!({"n": 2}))]
        );
    }
}
