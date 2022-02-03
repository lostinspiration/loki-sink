#![deny(missing_docs, unsafe_code)]
#![allow(clippy::tabs_in_doc_comments)]
//! An opinionated [Grafana loki](https://grafana.com/oss/loki/) logger for the [`log`](https://crates.io/crates/log) facade.

mod property_bag;
mod sink;

use log::LevelFilter;
use sink::{LokiLabels, LokiSink};
use std::thread;
use std::time::Duration;

pub use crate::property_bag::PROPERTY_BAG;
pub use log;

/// Convenience macro for adding a label to the property bag
#[macro_export]
macro_rules! prop {
	($name:expr, $object:expr) => {
		let _guard = $crate::PROPERTY_BAG.push($name, $object);
	};
}

/// Convenience macro for adding `CorrelationId` to the property bag
#[macro_export]
macro_rules! correlation_id {
	($object:expr) => {
		let _guard = $crate::PROPERTY_BAG.push("CorrelationId", $object);
	};
}

/// Convenience macro for adding `InstanceId` to the property bag
#[macro_export]
macro_rules! instance_id {
	($object:expr) => {
		let _guard = $crate::PROPERTY_BAG.push("InstanceId", $object);
	};
}

fn init_inner(url: impl AsRef<str>, max_log_level: LevelFilter, labels: LokiLabels) {
	log::set_boxed_logger(Box::new(LokiSink::new(url, labels)))
		.map(|_| {
			log::set_max_level(max_log_level);
		})
		.expect("failed to set logger");

	thread::spawn(|| {
		loop {
			thread::sleep(Duration::from_secs(1));
			log::logger().flush();
		}
	});
}

/// Initialize a new loki logger sink with a given level
pub fn init(url: impl AsRef<str>, max_log_level: LevelFilter) {
	init_inner(url, max_log_level, None);
}

/// Initialize a new loki logger sink with a given level and set of labels
pub fn init_with_labels(url: impl AsRef<str>, max_log_level: LevelFilter, labels: LokiLabels) {
	init_inner(url, max_log_level, labels);
}

#[cfg(test)]
mod tests {
	use super::*;
	use log::info;
	use std::collections::HashMap;

	#[test]
	fn it_works() {
		let mut initial_labels = HashMap::new();
		initial_labels.insert("Application", "ExampleConsoleApp");
		initial_labels.insert("Environment", "Stage");
		initial_labels.insert("MachineName", "ASURA");

		init_with_labels("http://localhost:3100/loki/api/v1/push", log::LevelFilter::Debug, Some(initial_labels));
		
		prop!("CorrelationId", &12345);
		info!("Test");
	}
}
