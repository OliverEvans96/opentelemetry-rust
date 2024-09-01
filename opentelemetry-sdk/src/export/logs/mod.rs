//! Log exporters
use crate::logs::LogRecord;
use crate::Resource;
use async_trait::async_trait;
#[cfg(feature = "logs_level_enabled")]
use opentelemetry::logs::Severity;
use opentelemetry::{
    logs::{LogError, LogResult},
    InstrumentationLibrary, MaybeBoxFuture,
};
use std::fmt::Debug;

/// `LogExporter` defines the interface that log exporters should implement.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait LogExporter: Send + Sync + Debug {
    /// Exports a batch of [`LogRecord`, `InstrumentationLibrary`].
    fn export(
        &mut self,
        batch: Vec<(&LogRecord, &InstrumentationLibrary)>,
    ) -> MaybeBoxFuture<'static, LogResult<()>>;
    /// Shuts down the exporter.
    fn shutdown(&mut self) {}
    #[cfg(feature = "logs_level_enabled")]
    /// Chek if logs are enabled.
    fn event_enabled(&self, _level: Severity, _target: &str, _name: &str) -> bool {
        // By default, all logs are enabled
        true
    }
    /// Set the resource for the exporter.
    fn set_resource(&mut self, _resource: &Resource) {}
}

/// Describes the result of an export.
pub type ExportResult = Result<(), LogError>;
