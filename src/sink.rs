use crate::{prop, PROPERTY_BAG};
use log::Log;
use serde::Serialize;
use std::{
	collections::HashMap,
	sync::RwLock,
	time::{SystemTime, UNIX_EPOCH},
};

pub type LokiLabels = Option<HashMap<&'static str, &'static str>>;
const BATCH_LIMIT: usize = 1000;

/// Top level request to push data to loki
///
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

/// Stream object that contains a list of labels and values
///
/// Note: The stream property is the collection of labels that loki will group together
/// and store in like chunks (files) on the server. You want this to be as low cardinality as possible to avoid
/// having too many individual files on the server
#[derive(Serialize, Clone, Debug)]
struct LokiStream {
	stream: HashMap<String, String>,
	values: Vec<(String, String)>,
}

/// Sink object that allows for writing to a [Grafana Loki](https://grafana.com/oss/loki/) implementation
///
/// Global labels define the cardinality of the log stream
///
/// Message properties are always serialized as json
///
/// #### Properties included by default
/// * Message
/// * LineNumber
/// * Target
/// * File
/// * level
#[derive(Debug)]
pub(crate) struct LokiSink {
	url: String,
	labels: HashMap<String, String>,
	buffer: RwLock<Vec<LokiStream>>,
}

impl LokiSink {
	pub(crate) fn new(url: impl AsRef<str>, labels: LokiLabels) -> Self {
		let initial_labels = match labels {
			Some(labels) => {
				labels
					.into_iter()
					.map(|(name, value)| {
						(String::from(name), String::from(value))
					})
					.collect()
			}
			None => HashMap::new()
		};

		Self {
			url: String::from(url.as_ref()),
			labels: initial_labels,
			buffer: RwLock::new(Vec::new()),
		}
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

		let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string();
		let message = format!("{:?}", record.args());

		// add standard set of properties to be logged
		prop!("Message", &message);
		prop!("LineNumber", &record.line());
		prop!("Target", &record.target());
		prop!("File", &record.file());
		prop!("level", &record.level().to_string().to_ascii_lowercase());

		let message_json = PROPERTY_BAG.as_json();
		let span = [(time, message_json)].to_vec();

		let req = LokiStream {
			stream: self.labels.clone(),
			values: span,
		};

		self.buffer.write().unwrap().push(req);

		// limit is hit, let's try to flush the logs to the server
		if self.buffer.read().unwrap().len() >= BATCH_LIMIT {
			self.flush();
		}
	}

	fn flush(&self) {
		let mut req = match self.buffer.try_write() {
			Ok(r) => r,
			// if we can't get a lock, well just try again next time flush is called
			Err(_) => return,
		};

		let batch_size = req.len().clamp(0, BATCH_LIMIT);
		if batch_size == 0 {
			return;
		}

		let payload = LokiRequest {
			streams: (*req).drain(..batch_size).collect(),
		};

		// we are done with the RwLock. drop it so that any logging in ureq and its dependencies
		// will not cause deadlocks
		drop(req);

		// for now just swallow and print to stderr
		// this can sometimes cause things to write to stderr if the program/thread execution
		// stops in the middle of making the call
		if let Err(e) = ureq::post(&self.url).send_json(payload) {
			eprintln!("{:?}", e);
		}
	}
}
