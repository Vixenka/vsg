use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ContentVariables {
    pub variables: HashMap<String, String>,
}

impl ContentVariables {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
    }
}
