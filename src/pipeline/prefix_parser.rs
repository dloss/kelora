use crate::event::Event;
use crate::pipeline::{EventParser, PrefixExtractor};
use anyhow::Result;

/// Wrapper parser that extracts prefix before parsing and adds it to the event
pub struct PrefixExtractingParser {
    inner: Box<dyn EventParser>,
    prefix_extractor: Option<PrefixExtractor>,
}

impl PrefixExtractingParser {
    pub fn new(inner: Box<dyn EventParser>, prefix_extractor: Option<PrefixExtractor>) -> Self {
        Self {
            inner,
            prefix_extractor,
        }
    }
}

impl EventParser for PrefixExtractingParser {
    fn parse(&self, line: &str) -> Result<Event> {
        let (modified_line, extracted_prefix) = if let Some(ref extractor) = self.prefix_extractor {
            extractor.extract_prefix(line)
        } else {
            (line.to_string(), None)
        };

        // Parse the line with the prefix removed
        let mut event = self.inner.parse(&modified_line)?;

        // Add the extracted prefix to the event if we have one
        if let Some(ref extractor) = self.prefix_extractor {
            extractor.add_prefix_to_event(&mut event, extracted_prefix);
        }

        Ok(event)
    }

    /// Prefix extraction only strips a leading segment of the raw line before
    /// handing the remainder to the inner parser; the level still comes from
    /// text that is a substring of the raw line. So verbatim-level extraction is
    /// whatever the inner parser guarantees.
    fn level_appears_verbatim(&self) -> bool {
        self.inner.level_appears_verbatim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::LineParser;

    #[test]
    fn test_prefix_extracting_parser() {
        let base_parser = Box::new(LineParser::new());
        let extractor = PrefixExtractor::new("src".to_string(), "|".to_string());
        let parser = PrefixExtractingParser::new(base_parser, Some(extractor));

        let result = parser.parse("web_1 | Test message").unwrap();

        assert_eq!(
            result
                .fields
                .get("line")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Test message"
        );
        assert_eq!(
            result
                .fields
                .get("src")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "web_1"
        );
    }

    #[test]
    fn test_prefix_extracting_parser_no_prefix() {
        let base_parser = Box::new(LineParser::new());
        let extractor = PrefixExtractor::new("src".to_string(), "|".to_string());
        let parser = PrefixExtractingParser::new(base_parser, Some(extractor));

        let result = parser.parse("Just a normal message").unwrap();

        assert_eq!(
            result
                .fields
                .get("line")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Just a normal message"
        );
        assert!(result.fields.get("src").is_none());
    }

    #[test]
    fn test_prefix_extracting_parser_no_extractor() {
        let base_parser = Box::new(LineParser::new());
        let parser = PrefixExtractingParser::new(base_parser, None);

        let result = parser.parse("web_1 | Test message").unwrap();

        assert_eq!(
            result
                .fields
                .get("line")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "web_1 | Test message"
        );
        assert!(result.fields.get("src").is_none());
    }
}
