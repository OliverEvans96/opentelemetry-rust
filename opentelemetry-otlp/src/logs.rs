//! OTLP - Log Exporter
//!
//! Defines a [LogExporter] to send logs via the OpenTelemetry Protocol (OTLP)

#[cfg(feature = "grpc-tonic")]
use crate::exporter::tonic::TonicExporterBuilder;

#[cfg(feature = "http-proto")]
use crate::exporter::http::HttpExporterBuilder;

use crate::{NoExporterConfig, OtlpPipeline};
use futures_core::future::BoxFuture;
use std::fmt::Debug;

use opentelemetry::logs::{LogError, LogResult};
use opentelemetry::InstrumentationLibrary;

use opentelemetry_sdk::{logs::LogRecord, runtime::RuntimeChannel, Resource};

/// Compression algorithm to use, defaults to none.
pub const OTEL_EXPORTER_OTLP_LOGS_COMPRESSION: &str = "OTEL_EXPORTER_OTLP_LOGS_COMPRESSION";

/// Target to which the exporter is going to send logs
pub const OTEL_EXPORTER_OTLP_LOGS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT";

/// Maximum time the OTLP exporter will wait for each batch logs export.
pub const OTEL_EXPORTER_OTLP_LOGS_TIMEOUT: &str = "OTEL_EXPORTER_OTLP_LOGS_TIMEOUT";

/// Key-value pairs to be used as headers associated with gRPC or HTTP requests
/// for sending logs.
/// Example: `k1=v1,k2=v2`
/// Note: this is only supported for HTTP.
pub const OTEL_EXPORTER_OTLP_LOGS_HEADERS: &str = "OTEL_EXPORTER_OTLP_LOGS_HEADERS";

impl OtlpPipeline {
    /// Create a OTLP logging pipeline.
    pub fn logging(self) -> OtlpLogPipeline<NoExporterConfig> {
        OtlpLogPipeline {
            resource: None,
            exporter_builder: NoExporterConfig(()),
            batch_config: None,
        }
    }
}

/// OTLP log exporter builder
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
#[non_exhaustive]
pub enum LogExporterBuilder {
    /// Tonic log exporter builder
    #[cfg(feature = "grpc-tonic")]
    Tonic(TonicExporterBuilder),
    /// Http log exporter builder
    #[cfg(feature = "http-proto")]
    Http(HttpExporterBuilder),
}

impl LogExporterBuilder {
    /// Build a OTLP log exporter using the given configuration.
    pub fn build_log_exporter(self) -> Result<LogExporter, LogError> {
        match self {
            #[cfg(feature = "grpc-tonic")]
            LogExporterBuilder::Tonic(builder) => builder.build_log_exporter(),
            #[cfg(feature = "http-proto")]
            LogExporterBuilder::Http(builder) => builder.build_log_exporter(),
        }
    }
}

#[cfg(feature = "grpc-tonic")]
impl From<TonicExporterBuilder> for LogExporterBuilder {
    fn from(exporter: TonicExporterBuilder) -> Self {
        LogExporterBuilder::Tonic(exporter)
    }
}

#[cfg(feature = "http-proto")]
impl From<HttpExporterBuilder> for LogExporterBuilder {
    fn from(exporter: HttpExporterBuilder) -> Self {
        LogExporterBuilder::Http(exporter)
    }
}

/// OTLP exporter that sends log data
#[derive(Debug)]
pub struct LogExporter {
    client: Box<dyn opentelemetry_sdk::export::logs::LogExporter>,
}

impl LogExporter {
    /// Create a new log exporter
    pub fn new(client: impl opentelemetry_sdk::export::logs::LogExporter + 'static) -> Self {
        LogExporter {
            client: Box::new(client),
        }
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl opentelemetry_sdk::export::logs::LogExporter for LogExporter {
    fn export(
        &mut self,
        batch: Vec<(&LogRecord, &InstrumentationLibrary)>,
    ) -> BoxFuture<'static, LogResult<()>> {
        Box::pin(self.client.export(batch))
    }

    fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
        self.client.set_resource(resource);
    }
}

/// Recommended configuration for an OTLP exporter pipeline.
#[derive(Debug)]
pub struct OtlpLogPipeline<EB> {
    exporter_builder: EB,
    resource: Option<Resource>,
    batch_config: Option<opentelemetry_sdk::logs::BatchConfig>,
}

impl<EB> OtlpLogPipeline<EB> {
    /// Set the Resource associated with log provider.
    pub fn with_resource(self, resource: Resource) -> Self {
        OtlpLogPipeline {
            resource: Some(resource),
            ..self
        }
    }

    /// Set the batch log processor configuration, and it will override the env vars.
    pub fn with_batch_config(mut self, batch_config: opentelemetry_sdk::logs::BatchConfig) -> Self {
        self.batch_config = Some(batch_config);
        self
    }
}

impl OtlpLogPipeline<NoExporterConfig> {
    /// Set the OTLP log exporter builder.
    pub fn with_exporter<B: Into<LogExporterBuilder>>(
        self,
        pipeline: B,
    ) -> OtlpLogPipeline<LogExporterBuilder> {
        OtlpLogPipeline {
            exporter_builder: pipeline.into(),
            resource: self.resource,
            batch_config: self.batch_config,
        }
    }
}

impl OtlpLogPipeline<LogExporterBuilder> {
    /// Install the configured log exporter.
    ///
    /// Returns a [`LoggerProvider`].
    ///
    /// [`LoggerProvider`]: opentelemetry_sdk::logs::LoggerProvider
    pub fn install_simple(self) -> Result<opentelemetry_sdk::logs::LoggerProvider, LogError> {
        Ok(build_simple_with_exporter(
            self.exporter_builder.build_log_exporter()?,
            self.resource,
        ))
    }

    /// Install the configured log exporter and a batch log processor using the
    /// specified runtime.
    ///
    /// Returns a [`LoggerProvider`].
    ///
    /// [`LoggerProvider`]: opentelemetry_sdk::logs::LoggerProvider
    pub fn install_batch<R: RuntimeChannel>(
        self,
        runtime: R,
    ) -> Result<opentelemetry_sdk::logs::LoggerProvider, LogError> {
        Ok(build_batch_with_exporter(
            self.exporter_builder.build_log_exporter()?,
            self.resource,
            runtime,
            self.batch_config,
        ))
    }
}

fn build_simple_with_exporter(
    exporter: LogExporter,
    resource: Option<Resource>,
) -> opentelemetry_sdk::logs::LoggerProvider {
    let mut provider_builder =
        opentelemetry_sdk::logs::LoggerProvider::builder().with_simple_exporter(exporter);
    if let Some(resource) = resource {
        provider_builder = provider_builder.with_resource(resource);
    }
    // logger would be created in the appenders like
    // opentelemetry-appender-tracing, opentelemetry-appender-log etc.
    provider_builder.build()
}

fn build_batch_with_exporter<R: RuntimeChannel>(
    exporter: LogExporter,
    resource: Option<Resource>,
    runtime: R,
    batch_config: Option<opentelemetry_sdk::logs::BatchConfig>,
) -> opentelemetry_sdk::logs::LoggerProvider {
    let mut provider_builder = opentelemetry_sdk::logs::LoggerProvider::builder();
    let batch_processor = opentelemetry_sdk::logs::BatchLogProcessor::builder(exporter, runtime)
        .with_batch_config(batch_config.unwrap_or_default())
        .build();
    provider_builder = provider_builder.with_log_processor(batch_processor);

    if let Some(resource) = resource {
        provider_builder = provider_builder.with_resource(resource);
    }
    // logger would be created in the tracing appender
    provider_builder.build()
}
