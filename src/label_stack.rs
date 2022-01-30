use once_cell::sync::Lazy;
use serde::Serialize;
use std::{collections::HashMap, sync::RwLock};

/// Property bag
pub static PROPERTY_BAG: Lazy<PropertyBag> = Lazy::new(|| PropertyBag::new());
type LabelStack = RwLock<HashMap<String, serde_json::Value>>;

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

	/// Serializes the properties in the `LabelStack` as json
	pub(crate) fn as_json(&self) -> String {
		serde_json::to_string(&self.labels.read().unwrap().clone()).unwrap()
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
