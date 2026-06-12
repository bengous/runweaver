use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Identifies one event source: a surface name (e.g. `"agent-hook"`), a
/// trigger name (e.g. `"post-edit"`), and an optional phase. Bindings match
/// triggers exactly, including the phase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceTrigger {
    pub surface: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// A decoded occurrence of a trigger: the trigger plus its JSON payload and
/// optional metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceEvent {
    pub trigger: SurfaceTrigger,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceResponseStatus {
    Success,
    Skipped,
    Blocked,
    Error,
}

/// What the surface reports back after handling an event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceResponse {
    pub status: SurfaceResponseStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

pub type SurfaceDecodeFn = Arc<dyn Fn(Value) -> SurfaceEvent + Send + Sync + 'static>;
pub type SurfaceEncodeFn = Arc<dyn Fn(SurfaceResponse) -> Value + Send + Sync + 'static>;

/// Bidirectional translation between a surface's native JSON protocol and
/// the normalized [`SurfaceEvent`]/[`SurfaceResponse`] pair.
#[derive(Clone)]
pub struct SurfaceCodec {
    pub decode: SurfaceDecodeFn,
    pub encode: SurfaceEncodeFn,
}

impl SurfaceCodec {
    pub fn new(
        decode: impl Fn(Value) -> SurfaceEvent + Send + Sync + 'static,
        encode: impl Fn(SurfaceResponse) -> Value + Send + Sync + 'static,
    ) -> Self {
        Self {
            decode: Arc::new(decode),
            encode: Arc::new(encode),
        }
    }
}

impl std::fmt::Debug for SurfaceCodec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceCodec")
            .field("decode", &"<fn>")
            .field("encode", &"<fn>")
            .finish()
    }
}

/// A named trigger with an optional codec; built with [`define_surface`].
#[derive(Debug, Clone)]
pub struct SurfaceDefinition {
    pub trigger: SurfaceTrigger,
    pub codec: Option<SurfaceCodec>,
}

impl SurfaceDefinition {
    pub fn trigger(&self) -> SurfaceTrigger {
        self.trigger.clone()
    }
}

pub fn define_surface(trigger: SurfaceTrigger, codec: Option<SurfaceCodec>) -> SurfaceDefinition {
    SurfaceDefinition { trigger, codec }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_surface_preserves_trigger() {
        let surface = define_surface(
            SurfaceTrigger {
                surface: "agent-hook".to_owned(),
                name: "post-edit".to_owned(),
                phase: Some("after".to_owned()),
            },
            None,
        );

        assert_eq!(surface.trigger().name, "post-edit");
    }
}
