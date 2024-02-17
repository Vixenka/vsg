use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ContentVariables {
    pub variables: HashMap<String, Vec<u8>>,
}

impl ContentVariables {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: String, value: Vec<u8>) {
        self.variables.insert(key, value);
    }
}
