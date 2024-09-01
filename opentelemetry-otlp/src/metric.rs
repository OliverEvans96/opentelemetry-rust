//! OTEL metric exporter
//!
//! Defines a [MetricsExporter] to send metric data to backend via OTLP protocol.
//!

use crate::{NoExporterConfig, OtlpPipeline};
use async_trait::async_trait;
use core::fmt;
use opentelemetry::metrics::Result;

#[cfg(feature = "grpc-tonic")]
use crate::exporter::tonic::TonicExporterBuilder;
use opentelemetry_sdk::{
    metrics::{
        data::{ResourceMetrics, Temporality},
        exporter::PushMetricsExporter,
        reader::{
            AggregationSelector, DefaultAggregationSelector, DefaultTemporalitySelector,
            TemporalitySelector,
        },
        Aggregation, InstrumentKind, PeriodicReader, SdkMeterProvider,
    },
    runtime::Runtime,
    Resource,
};
use std::fmt::{Debug, Formatter};
use std::time;

#[cfg(feature = "http-proto")]
use crate::exporter::http::HttpExporterBuilder;

/// Target to which the exporter is going to send metrics, defaults to https://localhost:4317/v1/metrics.
/// Learn about the relationship between this constant and default/spans/logs at
/// <https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/protocol/exporter.md#endpoint-urls-for-otlphttp>
pub const OTEL_EXPORTER_OTLP_METRICS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT";
/// Max waiting time for the backend to process each metrics batch, defaults to 10s.
pub const OTEL_EXPORTER_OTLP_METRICS_TIMEOUT: &str = "OTEL_EXPORTER_OTLP_METRICS_TIMEOUT";
/// Compression algorithm to use, defaults to none.
pub const OTEL_EXPORTER_OTLP_METRICS_COMPRESSION: &str = "OTEL_EXPORTER_OTLP_METRICS_COMPRESSION";
/// Key-value pairs to be used as headers associated with gRPC or HTTP requests
/// for sending metrics.
/// Example: `k1=v1,k2=v2`
/// Note: this is only supported for HTTP.
pub const OTEL_EXPORTER_OTLP_METRICS_HEADERS: &str = "OTEL_EXPORTER_OTLP_METRICS_HEADERS";
impl OtlpPipeline {
    /// Create a OTLP metrics pipeline.
    pub fn metrics<RT>(self, rt: RT) -> OtlpMetricPipeline<RT, NoExporterConfig>
    where
        RT: Runtime,
    {
        OtlpMetricPipeline {
            rt,
            aggregator_selector: None,
            temporality_selector: None,
            exporter_pipeline: NoExporterConfig(()),
            resource: None,
            period: None,
            timeout: None,
        }
    }
}

/// OTLP metrics exporter builder.
#[derive(Debug)]
#[non_exhaustive]
pub enum MetricsExporterBuilder {
    /// Tonic metrics exporter builder
    #[cfg(feature = "grpc-tonic")]
    Tonic(TonicExporterBuilder),
    /// Http metrics exporter builder
    #[cfg(feature = "http-proto")]
    Http(HttpExporterBuilder),

    /// Missing exporter builder
    #[doc(hidden)]
    #[cfg(not(any(feature = "http-proto", feature = "grpc-tonic")))]
    Unconfigured,
}

impl MetricsExporterBuilder {
    /// Build a OTLP metrics exporter with given configuration.
    pub fn build_metrics_exporter(
        self,
        temporality_selector: Box<dyn TemporalitySelector>,
        aggregation_selector: Box<dyn AggregationSelector>,
    ) -> Result<MetricsExporter> {
        match self {
            #[cfg(feature = "grpc-tonic")]
            MetricsExporterBuilder::Tonic(builder) => {
                builder.build_metrics_exporter(aggregation_selector, temporality_selector)
            }
            #[cfg(feature = "http-proto")]
            MetricsExporterBuilder::Http(builder) => {
                builder.build_metrics_exporter(aggregation_selector, temporality_selector)
            }
            #[cfg(not(any(feature = "http-proto", feature = "grpc-tonic")))]
            MetricsExporterBuilder::Unconfigured => {
                drop(temporality_selector);
                drop(aggregation_selector);
                Err(opentelemetry::metrics::MetricsError::Other(
                    "no configured metrics exporter, enable `http-proto` or `grpc-tonic` feature to configure a metrics exporter".into(),
                ))
            }
        }
    }
}

#[cfg(feature = "grpc-tonic")]
impl From<TonicExporterBuilder> for MetricsExporterBuilder {
    fn from(exporter: TonicExporterBuilder) -> Self {
        MetricsExporterBuilder::Tonic(exporter)
    }
}

#[cfg(feature = "http-proto")]
impl From<HttpExporterBuilder> for MetricsExporterBuilder {
    fn from(exporter: HttpExporterBuilder) -> Self {
        MetricsExporterBuilder::Http(exporter)
    }
}

/// Pipeline to build OTLP metrics exporter
///
/// Note that currently the OTLP metrics exporter only supports tonic as it's grpc layer and tokio as
/// runtime.
pub struct OtlpMetricPipeline<RT, EB> {
    rt: RT,
    aggregator_selector: Option<Box<dyn AggregationSelector>>,
    temporality_selector: Option<Box<dyn TemporalitySelector>>,
    exporter_pipeline: EB,
    resource: Option<Resource>,
    period: Option<time::Duration>,
    timeout: Option<time::Duration>,
}

impl<RT, EB> OtlpMetricPipeline<RT, EB>
where
    RT: Runtime,
{
    /// Build with resource key value pairs.
    pub fn with_resource(self, resource: Resource) -> Self {
        OtlpMetricPipeline {
            resource: Some(resource),
            ..self
        }
    }

    /// Build with timeout
    pub fn with_timeout(self, timeout: time::Duration) -> Self {
        OtlpMetricPipeline {
            timeout: Some(timeout),
            ..self
        }
    }

    /// Build with period, your metrics will be exported with this period
    pub fn with_period(self, period: time::Duration) -> Self {
        OtlpMetricPipeline {
            period: Some(period),
            ..self
        }
    }

    /// Build with the given temporality selector
    pub fn with_temporality_selector<T: TemporalitySelector + 'static>(self, selector: T) -> Self {
        OtlpMetricPipeline {
            temporality_selector: Some(Box::new(selector)),
            ..self
        }
    }

    /// Build with delta temporality selector.
    ///
    /// This temporality selector is equivalent to OTLP Metrics Exporter's
    /// `Delta` temporality preference (see [its documentation][exporter-docs]).
    ///
    /// [exporter-docs]: https://github.com/open-telemetry/opentelemetry-specification/blob/a1c13d59bb7d0fb086df2b3e1eaec9df9efef6cc/specification/metrics/sdk_exporters/otlp.md#additional-configuration
    pub fn with_delta_temporality(self) -> Self {
        self.with_temporality_selector(DeltaTemporalitySelector)
    }

    /// Build with the given aggregation selector
    pub fn with_aggregation_selector<T: AggregationSelector + 'static>(self, selector: T) -> Self {
        OtlpMetricPipeline {
            aggregator_selector: Some(Box::new(selector)),
            ..self
        }
    }
}

impl<RT> OtlpMetricPipeline<RT, NoExporterConfig>
where
    RT: Runtime,
{
    /// Build with the exporter
    pub fn with_exporter<B: Into<MetricsExporterBuilder>>(
        self,
        pipeline: B,
    ) -> OtlpMetricPipeline<RT, MetricsExporterBuilder> {
        OtlpMetricPipeline {
            exporter_pipeline: pipeline.into(),
            rt: self.rt,
            aggregator_selector: self.aggregator_selector,
            temporality_selector: self.temporality_selector,
            resource: self.resource,
            period: self.period,
            timeout: self.timeout,
        }
    }
}

impl<RT> OtlpMetricPipeline<RT, MetricsExporterBuilder>
where
    RT: Runtime,
{
    /// Build MeterProvider
    pub fn build(self) -> Result<SdkMeterProvider> {
        let exporter = self.exporter_pipeline.build_metrics_exporter(
            self.temporality_selector
                .unwrap_or_else(|| Box::new(DefaultTemporalitySelector::new())),
            self.aggregator_selector
                .unwrap_or_else(|| Box::new(DefaultAggregationSelector::new())),
        )?;

        let mut builder = PeriodicReader::builder(exporter, self.rt);

        if let Some(period) = self.period {
            builder = builder.with_interval(period);
        }
        if let Some(timeout) = self.timeout {
            builder = builder.with_timeout(timeout)
        }

        let reader = builder.build();

        let mut provider = SdkMeterProvider::builder().with_reader(reader);

        if let Some(resource) = self.resource {
            provider = provider.with_resource(resource);
        }

        let provider = provider.build();
        Ok(provider)
    }
}

impl<RT, EB: Debug> Debug for OtlpMetricPipeline<RT, EB> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtlpMetricPipeline")
            .field("exporter_pipeline", &self.exporter_pipeline)
            .field("resource", &self.resource)
            .field("period", &self.period)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// A temporality selector that returns [`Delta`][Temporality::Delta] for all
/// instruments except `UpDownCounter` and `ObservableUpDownCounter`.
///
/// This temporality selector is equivalent to OTLP Metrics Exporter's
/// `Delta` temporality preference (see [its documentation][exporter-docs]).
///
/// [exporter-docs]: https://github.com/open-telemetry/opentelemetry-specification/blob/a1c13d59bb7d0fb086df2b3e1eaec9df9efef6cc/specification/metrics/sdk_exporters/otlp.md#additional-configuration
#[derive(Debug)]
struct DeltaTemporalitySelector;

impl TemporalitySelector for DeltaTemporalitySelector {
    #[rustfmt::skip]
    fn temporality(&self, kind: InstrumentKind) -> Temporality {
        match kind {
            InstrumentKind::Counter
            | InstrumentKind::Histogram
            | InstrumentKind::ObservableCounter
            | InstrumentKind::Gauge
            | InstrumentKind::ObservableGauge => {
                Temporality::Delta
            }
            InstrumentKind::UpDownCounter
            | InstrumentKind::ObservableUpDownCounter => {
                Temporality::Cumulative
            }
        }
    }
}

/// An interface for OTLP metrics clients
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait MetricsClient: fmt::Debug + Send + Sync + 'static {
    async fn export(&self, metrics: &mut ResourceMetrics) -> Result<()>;
    fn shutdown(&self) -> Result<()>;
}

/// Export metrics in OTEL format.
pub struct MetricsExporter {
    client: Box<dyn MetricsClient>,
    temporality_selector: Box<dyn TemporalitySelector>,
    aggregation_selector: Box<dyn AggregationSelector>,
}

impl Debug for MetricsExporter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsExporter").finish()
    }
}

impl TemporalitySelector for MetricsExporter {
    fn temporality(&self, kind: InstrumentKind) -> Temporality {
        self.temporality_selector.temporality(kind)
    }
}

impl AggregationSelector for MetricsExporter {
    fn aggregation(&self, kind: InstrumentKind) -> Aggregation {
        self.aggregation_selector.aggregation(kind)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl PushMetricsExporter for MetricsExporter {
    async fn export(&self, metrics: &mut ResourceMetrics) -> Result<()> {
        self.client.export(metrics).await
    }

    async fn force_flush(&self) -> Result<()> {
        // this component is stateless
        Ok(())
    }

    fn shutdown(&self) -> Result<()> {
        self.client.shutdown()
    }
}

impl MetricsExporter {
    /// Create a new metrics exporter
    pub fn new(
        client: impl MetricsClient,
        temporality_selector: Box<dyn TemporalitySelector>,
        aggregation_selector: Box<dyn AggregationSelector>,
    ) -> MetricsExporter {
        MetricsExporter {
            client: Box::new(client),
            temporality_selector,
            aggregation_selector,
        }
    }
}
