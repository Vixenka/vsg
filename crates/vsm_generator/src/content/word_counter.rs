use super::{content_variables::ContentVariables, preliminary_analysis::Content};

pub fn compute_read_time(
    file_content: &str,
    content: &mut Content,
    variables: &mut ContentVariables,
) {
    let Content::Blog(content) = content else {
        return;
    };

    let word_count = words_count::count(file_content).words as u64;
    variables.insert("md_word_count".to_owned(), word_count.to_string());

    // TODO: Use beter algorithm to calculate read time
    let wpm = 240.0 - (content.difficulty * 15.0);
    let read_time = (word_count as f64 / wpm) * 60.0;

    variables.insert(
        "md_read_time".to_owned(),
        (read_time / 60.0).round().to_string(),
    );
}
