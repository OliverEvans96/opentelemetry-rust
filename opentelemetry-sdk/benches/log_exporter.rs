/*
    The benchmark results:
    criterion = "0.5.1"
    OS: Ubuntu 22.04.3 LTS (5.15.146.1-microsoft-standard-WSL2)
    Hardware: AMD EPYC 7763 64-Core Processor - 2.44 GHz, 16vCPUs,
    RAM: 64.0 GB
    | Test                           | Average time|
    |--------------------------------|-------------|
    | LogExporterWithFuture          | 280 ns      |
    | LogExporterWithoutFuture       | 255 ns      |
*/

use std::sync::Mutex;
use std::time::SystemTime;

use criterion::{criterion_group, criterion_main, Criterion};

use futures_util::future::BoxFuture;
use opentelemetry::logs::{LogRecord as _, LogResult, Logger as _, LoggerProvider as _, Severity};

use opentelemetry::InstrumentationLibrary;
use opentelemetry_sdk::logs::LogProcessor;
use opentelemetry_sdk::logs::LogRecord;
use opentelemetry_sdk::logs::LoggerProvider;
use pprof::criterion::{Output, PProfProfiler};
use std::fmt::Debug;

// Run this benchmark with:
// cargo bench --bench log_exporter
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait LogExporterWithFuture: Send + Sync + Debug {
    fn export(
        &mut self,
        batch: Vec<(&LogRecord, &InstrumentationLibrary)>,
    ) -> BoxFuture<'static, ()>;
}

pub trait LogExporterWithoutFuture: Send + Sync + Debug {
    fn export(&mut self, batch: Vec<(&LogRecord, &InstrumentationLibrary)>);
}

#[derive(Debug)]
struct NoOpExporterWithFuture {}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl LogExporterWithFuture for NoOpExporterWithFuture {
    fn export(
        &mut self,
        _batch: Vec<(&LogRecord, &InstrumentationLibrary)>,
    ) -> BoxFuture<'static, ()> {
        Box::pin(std::future::ready(()))
    }
}

#[derive(Debug)]
struct NoOpExporterWithoutFuture {}
impl LogExporterWithoutFuture for NoOpExporterWithoutFuture {
    fn export(&mut self, _batch: Vec<(&LogRecord, &InstrumentationLibrary)>) {}
}

#[derive(Debug)]
struct ExportingProcessorWithFuture {
    exporter: Mutex<NoOpExporterWithFuture>,
}

impl ExportingProcessorWithFuture {
    fn new(exporter: NoOpExporterWithFuture) -> Self {
        ExportingProcessorWithFuture {
            exporter: Mutex::new(exporter),
        }
    }
}

#[async_trait::async_trait]
impl LogProcessor for ExportingProcessorWithFuture {
    async fn emit(&self, record: &mut LogRecord, library: &InstrumentationLibrary) {
        let export_future = self
            .exporter
            .lock()
            .expect("lock error")
            .export(vec![(record, library)]);
        export_future.await;
    }

    async fn force_flush(&self) -> LogResult<()> {
        Ok(())
    }

    async fn shutdown(&self) -> LogResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct ExportingProcessorWithoutFuture {
    exporter: Mutex<NoOpExporterWithoutFuture>,
}

impl ExportingProcessorWithoutFuture {
    fn new(exporter: NoOpExporterWithoutFuture) -> Self {
        ExportingProcessorWithoutFuture {
            exporter: Mutex::new(exporter),
        }
    }
}

#[async_trait::async_trait]
impl LogProcessor for ExportingProcessorWithoutFuture {
    async fn emit(&self, record: &mut LogRecord, library: &InstrumentationLibrary) {
        self.exporter
            .lock()
            .expect("lock error")
            .export(vec![(record, library)]);
    }

    async fn force_flush(&self) -> LogResult<()> {
        Ok(())
    }

    async fn shutdown(&self) -> LogResult<()> {
        Ok(())
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    exporter_with_future(c);
    exporter_without_future(c);
}

fn exporter_with_future(c: &mut Criterion) {
    let provider = LoggerProvider::builder()
        .with_log_processor(ExportingProcessorWithFuture::new(NoOpExporterWithFuture {}))
        .build();
    let logger = provider.logger("benchmark");

    c.bench_function("LogExporterWithFuture", |b| {
        b.iter(|| {
            let mut log_record = logger.create_log_record();
            let now = SystemTime::now();
            log_record.set_observed_timestamp(now);
            log_record.set_target("my-target".to_string());
            log_record.set_event_name("CheckoutFailed");
            log_record.set_severity_number(Severity::Warn);
            log_record.set_severity_text("WARN");
            log_record.add_attribute("book_id", "12345");
            log_record.add_attribute("book_title", "Rust Programming Adventures");
            log_record.add_attribute("message", "Unable to process checkout.");

            logger.emit(log_record);
        });
    });
}

fn exporter_without_future(c: &mut Criterion) {
    let provider = LoggerProvider::builder()
        .with_log_processor(ExportingProcessorWithoutFuture::new(
            NoOpExporterWithoutFuture {},
        ))
        .build();
    let logger = provider.logger("benchmark");

    c.bench_function("LogExporterWithoutFuture", |b| {
        b.iter(|| {
            let mut log_record = logger.create_log_record();
            let now = SystemTime::now();
            log_record.set_observed_timestamp(now);
            log_record.set_target("my-target".to_string());
            log_record.set_event_name("CheckoutFailed");
            log_record.set_severity_number(Severity::Warn);
            log_record.set_severity_text("WARN");
            log_record.add_attribute("book_id", "12345");
            log_record.add_attribute("book_title", "Rust Programming Adventures");
            log_record.add_attribute("message", "Unable to process checkout.");

            logger.emit(log_record);
        });
    });
}

#[cfg(not(target_os = "windows"))]
criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = criterion_benchmark
}
#[cfg(target_os = "windows")]
criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = criterion_benchmark
}
criterion_main!(benches);
