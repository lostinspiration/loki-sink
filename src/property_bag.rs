use once_cell::sync::Lazy;
use serde::Serialize;
use std::{collections::HashMap, sync::RwLock};

/// Property bag
pub static PROPERTY_BAG: Lazy<PropertyBag> = Lazy::new(PropertyBag::new);
type PropertyStack = RwLock<HashMap<String, serde_json::Value>>;

/// Property bag that hold the `PropertyStack` used to enrich the logs written to loki
#[derive(Debug)]
pub struct PropertyBag {
	props: PropertyStack,
}

impl PropertyBag {
	/// Initializes a new `PropertyBag`
	fn new() -> Self {
		PropertyBag {
			props: RwLock::new(HashMap::new()),
		}
	}

	/// Pushes a new label and its corresponding data onto the `PropertyStack` in the `PropertyBag`
	///
	/// ```rust
	/// // keep the guard around otherwise the property will be dropped immediately
	/// // and will not propagate to loki with any log messages
	/// let _guard = loki_sink::PROPERTY_BAG.push("LabelName", &"LabelValue/Object");
	/// ```
	pub fn push<T: Serialize>(&self, name: &str, object: &T) -> PropertyStackGuard {
		let t = serde_json::to_value(object).unwrap();
		self.props.write().unwrap().insert(String::from(name), t);

		PropertyStackGuard {
			key: String::from(name),
			props: &self.props,
		}
	}

	/// Serializes the properties in the `PropertyStack` as json
	pub(crate) fn as_json(&self) -> String {
		serde_json::to_string(&self.props.read().unwrap().clone()).unwrap()
	}
}

/// Guard object with a drop implementation that will remove the guarded property
/// from the label stack at the end of the scope
pub struct PropertyStackGuard<'a> {
	key: String,
	props: &'a PropertyStack,
}

impl Drop for PropertyStackGuard<'_> {
	fn drop(&mut self) {
		self.props.write().unwrap().remove(&self.key);
	}
}
