use std::{collections::HashMap, ops::Range, sync::Arc};

use crate::Context;

use super::ContentResult;

#[derive(Debug, Default, Clone)]
pub struct ContentVariables {
    pub variables: HashMap<String, String>,
}

impl ContentVariables {
    pub fn new() -> Self {
        let mut variables = HashMap::new();
        variables.insert("warning".to_owned(), String::new());
        Self { variables }
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
    }

    pub fn apply(
        &mut self,
        data: &mut String,
        mut range: Range<usize>,
        context: &Arc<Context>,
        result: &mut ContentResult,
    ) {
        while let Some(start) = data[range.start..range.end].find("{{") {
            range.start += start;
            let mut end = match data[range.start..range.end].find("}}") {
                Some(end) => range.start + end + 2,
                None => {
                    result.push_error(anyhow::anyhow!(
                        "Unable to find end of variable. In position {}.",
                        range.start
                    ));
                    return;
                }
            };

            let mut key = &data[range.start + 2..end - 2];
            if let Some(set) = key.find(':') {
                let mut key_start = range.start + set + 3;
                while let Some(s) = data[key_start..end - 2].find("{{") {
                    key_start += s + 2;
                    end = match data[end..range.end].find("}}") {
                        Some(new_end) => end + new_end + 2,
                        None => {
                            result.push_error(anyhow::anyhow!(
                                "Unable to find end of variable. In position {}.",
                                range.start
                            ));
                            return;
                        }
                    };

                    key = &data[range.start + 2..end - 2];
                }

                let set_key = &key[..set];
                let set_value = &key[set + 1..];

                self.variables
                    .insert(set_key.to_string(), set_value.to_string());

                data.replace_range(range.start..end, "");

                range.end -= end - range.start;
                continue;
            }

            let variable_content = match key {
                "md_post_list" => context.md_post_list.get().unwrap(),
                _ => match self.variables.get(key) {
                    Some(variable_content) => variable_content,
                    None => {
                        result.push_error(anyhow::anyhow!(
                            "Unable to find variable with key '{}'",
                            key
                        ));
                        return;
                    }
                },
            };

            data.replace_range(range.start..end, variable_content);

            range.end = range.end + variable_content.len() - (end - range.start);
            range.start += variable_content.len();
        }
    }
}
