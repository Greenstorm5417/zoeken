//! Strip tracker query parameters from result URLs before they reach the client.

use zoeken_data::TrackerPatterns;
use zoeken_results::Result_;
use zoeken_search::ResultContainer;

pub(crate) fn strip_trackers(container: &mut ResultContainer, patterns: &TrackerPatterns) {
    for result in &mut container.results {
        match result {
            Result_::Main(r) => {
                r.url = patterns.clean_url(&r.url);
                if !r.thumbnail.is_empty() {
                    r.thumbnail = patterns.clean_url(&r.thumbnail);
                }
            }
            Result_::Image(r) => {
                r.url = patterns.clean_url(&r.url);
                r.img_src = patterns.clean_url(&r.img_src);
                if !r.thumbnail_src.is_empty() {
                    r.thumbnail_src = patterns.clean_url(&r.thumbnail_src);
                }
            }
            Result_::Paper(r) => r.url = patterns.clean_url(&r.url),
            Result_::Code(r) => r.url = patterns.clean_url(&r.url),
            Result_::File(r) => r.url = patterns.clean_url(&r.url),
            Result_::KeyValue(r) => r.url = patterns.clean_url(&r.url),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoeken_results::MainResult;

    #[test]
    fn strips_tracker_params_from_main_results() {
        let mut container = ResultContainer {
            results: vec![Result_::Main(MainResult {
                url: "https://example.com/?utm_source=x&keep=1".to_string(),
                ..MainResult::default()
            })],
            ..ResultContainer::default()
        };
        let patterns = zoeken_data::load_embedded_bundle()
            .expect("embedded data")
            .tracker_patterns;
        strip_trackers(&mut container, &patterns);
        let Result_::Main(result) = &container.results[0] else {
            unreachable!()
        };
        assert!(!result.url.contains("utm_source"));
        assert!(result.url.contains("keep=1"));
    }
}
