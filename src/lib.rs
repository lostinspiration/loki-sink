#![allow(dead_code, non_upper_case_globals, unused_imports, unused_macros)]

use log::{Log, log_enabled};
use once_cell::sync::{Lazy, OnceCell};
use serde::Serialize;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::thread;
use std::{collections::HashMap, sync::Arc};

pub use log;

pub static PROPERTY_BAG: Lazy<PropertyBag> = Lazy::new(|| PropertyBag::new());
static BUFFER_FLUSH_LIMIT: usize = 1000;

/// Convenience macro for adding a label to the logging stack
#[macro_export]
macro_rules! add_property {
	($name:expr, $object:expr) => {
		let _guard = $crate::PROPERTY_BAG.push($name, $object);
	};
}

type LabelStack = RwLock<HashMap<String, serde_json::Value>>;

/// Initialize a new loki logger sink
pub fn init(url: impl AsRef<str>, global_labels: Option<HashMap<&'static str, &'static str>>) {
	let logger = Box::new(LokiSink::new(url, global_labels));
	log::set_boxed_logger(logger)
		.map(|_| {
			log::set_max_level(log::LevelFilter::Debug);
		})
		.expect("failed to set logger");

	// thread::spawn(|| {
		
	// });
}

/// Property bag that hold the label stack used to enrich the logs written to loki
#[derive(Debug)]
pub struct PropertyBag {
	labels: LabelStack,
}

impl PropertyBag {
	/// Initializes a new `PropertyBag`
	fn new() -> Self {
		PropertyBag {
			labels: RwLock::new(HashMap::new()),
		}
	}

	/// Pushes a new label and its corresponding data onto the `LabelStack` in the `PropertyBag`
	///
	/// ```rust
	/// // keep the guard around otherwise the property will be dropped immediately
	/// // and will not propagate to loki with any log messages
	/// let _guard = loki_sink::PROPERTY_BAG.push("LabelName", &"LabelValue/Object");
	/// ```
	pub fn push<T: Serialize>(&self, name: &str, object: &T) -> LabelStackGuard {
		let t = serde_json::to_value(object).expect("unable to serialize");
		self.labels.write().unwrap().insert(String::from(name), t);

		LabelStackGuard {
			key: String::from(name),
			labels: &self.labels,
		}
	}
}

/// Guard object with a drop implementation that will remove the guarded property
/// from the label stack at the end of the scope
pub struct LabelStackGuard<'a> {
	key: String,
	labels: &'a LabelStack,
}

impl Drop for LabelStackGuard<'_> {
	fn drop(&mut self) {
		self.labels.write().unwrap().remove(&self.key);
	}
}

/// ```json
/// {
/// 	"streams": [
/// 	{
/// 		"stream": {
/// 			"label": "value"
/// 		},
/// 		"values": [
/// 			[ "unix epoch in nanoseconds", "log line" ],
/// 			[ "unix epoch in nanoseconds", "log line" ]
/// 		]
/// 	}
/// 	]
/// }
/// ```
#[derive(Serialize, Debug)]
struct LokiRequest {
	streams: Vec<LokiStream>,
}

#[derive(Serialize, Clone, Debug)]
struct LokiStream {
	stream: HashMap<String, String>,
	values: Vec<(String, String)>,
}

/// Sink object that allows for writing to a Grafana Loki implementation
pub struct LokiSink {
	url: String,
	global_labels: Option<HashMap<&'static str, &'static str>>,
	buffer: RwLock<Vec<LokiRequest>>,
}

impl LokiSink {
	fn new(url: impl AsRef<str>, global_labels: Option<HashMap<&'static str, &'static str>>) -> Self {
		Self { 
			url: url.as_ref().to_string(), 
			global_labels,
			buffer: RwLock::new(Vec::new()),
		}
	}

	fn log_event_record(&self, record: &log::Record) -> Result<(), Box<dyn Error>> {
		let message = format!("{:?}", record.args());

		let time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_nanos()
			.to_string();

		add_property!("Message", &message);
		add_property!("LineNumber", &record.line());
		add_property!("Target", &record.target());
		add_property!("File", &record.file());
		add_property!("level", &record.level().to_string().to_ascii_lowercase());

		let message_json = serde_json::to_string(&PROPERTY_BAG.labels.read().unwrap().clone()).unwrap();
		let span = [(time, message_json)];
		let globals = self.global_labels.as_ref().unwrap().into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();

		let req = LokiRequest {
			streams: [LokiStream {
				stream: globals,
				values: span.to_vec(),
			}].to_vec(),
		};

		self.buffer.write().unwrap().push(req);

		Ok(())
	}
}

impl Log for LokiSink {
	fn enabled(&self, _metadata: &log::Metadata) -> bool {
		true
	}

	fn log(&self, record: &log::Record) {
		if !self.enabled(record.metadata()) {
			return;
		}

		// ureq and its dependencies use the log facade and as such overflow the call stack
		// just block everything from its root for the time being.
		// XXX: not happy about this but it works for now
		if record.target().contains("ureq") {
			return;
		}

		if let Err(e) = self.log_event_record(record) {
			eprintln!("{:?}", e);
		}

		// limit is hit, let's try to flush the logs to the server
		if self.buffer.read().unwrap().len() >= BUFFER_FLUSH_LIMIT {
			self.flush();
		}
	}

	fn flush(&self) {
		let mut req = match self.buffer.try_write() {
			Ok(r) => r,
			// if we can't get a lock, well just try again next time flush is called
			Err(_) => return
		};

		let payload = LokiRequest {
			streams: (*req).drain(..).map(|a| a.streams).flatten().collect() 
		};

		// for now just swallow the error and print to stderr
		if let Err(e) = ureq::post(&self.url).send_json(payload) {
			eprintln!("{:?}", e);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use log::{debug, info};

	#[derive(Serialize)]
	struct Sample {
		name: String,
		age: u8,
		weight: u16,
	}

	#[test]
	fn it_works() {
		let mut initial_labels = HashMap::new();
		initial_labels.insert("Application", "ExampleConsoleApp");
		initial_labels.insert("Environment", "Stage");
		initial_labels.insert("MachineName", "ASURA");

		init("http://localhost:3100/loki/api/v1/push", Some(initial_labels));

		add_property!("CorrelationId", &12345);
		info!("Test");
	}
}
