use crate::event::Event;
use crate::parsers::type_conversion::{
    convert_value_to_type, parse_field_with_type, FieldType, TypeMap,
};
use crate::pipeline::EventParser;
use anyhow::{Context, Result};
use csv::ReaderBuilder;
use rhai::Dynamic;

/// CSV/TSV Parser that supports multiple dialects
///
/// Supported formats:
/// - CSV: Comma-separated values with headers
/// - TSV: Tab-separated values with headers
/// - CSVNH: Comma-separated values without headers (generates c1, c2, c3...)
/// - TSVNH: Tab-separated values without headers (generates c1, c2, c3...)
///
/// Ragged rows are preserved, not dropped: columns beyond the known headers are
/// kept under positional names (cN), rows with fewer columns leave the trailing
/// fields absent, and both cases are counted as diagnostics. With --strict, a
/// ragged row is a parse error instead.
pub struct CsvParser {
    delimiter: u8,
    has_headers: bool,
    headers: Vec<String>,
    type_map: TypeMap,
    strict: bool,
    auto_timestamp: bool,
}

impl CsvParser {
    /// Create a new CSV parser (comma-separated with headers)
    pub fn new_csv() -> Self {
        Self {
            delimiter: b',',
            has_headers: true,
            headers: Vec::new(),
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Create a new TSV parser (tab-separated with headers)
    pub fn new_tsv() -> Self {
        Self {
            delimiter: b'\t',
            has_headers: true,
            headers: Vec::new(),
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Create a new CSV parser without headers (comma-separated, generates c1, c2, c3...)
    pub fn new_csv_no_headers() -> Self {
        Self {
            delimiter: b',',
            has_headers: false,
            headers: Vec::new(),
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Create a new TSV parser without headers (tab-separated, generates c1, c2, c3...)
    pub fn new_tsv_no_headers() -> Self {
        Self {
            delimiter: b'\t',
            has_headers: false,
            headers: Vec::new(),
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Create a CSV parser with pre-initialized headers (for parallel processing)
    pub fn new_csv_with_headers(headers: Vec<String>) -> Self {
        Self {
            delimiter: b',',
            has_headers: true,
            headers,
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Create a TSV parser with pre-initialized headers (for parallel processing)
    pub fn new_tsv_with_headers(headers: Vec<String>) -> Self {
        Self {
            delimiter: b'\t',
            has_headers: true,
            headers,
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Create a CSV parser without headers but with pre-generated column names
    pub fn new_csv_no_headers_with_columns(headers: Vec<String>) -> Self {
        Self {
            delimiter: b',',
            has_headers: false,
            headers,
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Create a TSV parser without headers but with pre-generated column names
    pub fn new_tsv_no_headers_with_columns(headers: Vec<String>) -> Self {
        Self {
            delimiter: b'\t',
            has_headers: false,
            headers,
            type_map: TypeMap::new(),
            strict: false,
            auto_timestamp: true,
        }
    }

    /// Get the headers (for extracting initialized headers)
    pub fn get_headers(&self) -> Vec<String> {
        self.headers.clone()
    }

    /// Get the type map (for extracting initialized types)
    pub fn get_type_map(&self) -> TypeMap {
        self.type_map.clone()
    }

    /// Apply a pre-initialized type map
    pub fn with_type_map(mut self, type_map: TypeMap) -> Self {
        self.type_map = type_map;
        self
    }

    /// Parse a field spec string and populate the type map
    /// Field spec format: "field1:int field2:float field3:bool field4"
    pub fn with_field_spec(mut self, field_spec: &str) -> Result<Self> {
        for spec in field_spec.split_whitespace() {
            let (field_name, field_type) = parse_field_with_type(spec)
                .map_err(|e| anyhow::anyhow!("Invalid field spec '{}': {}", spec, e))?;

            if let Some(ftype) = field_type {
                self.type_map.insert(field_name, ftype);
            }
        }
        Ok(self)
    }

    /// Set strict mode for type conversion
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    pub fn with_auto_timestamp(mut self, auto_timestamp: bool) -> Self {
        self.auto_timestamp = auto_timestamp;
        self
    }

    /// Initialize headers from the first line if needed
    pub fn initialize_headers_from_line(&mut self, line: &str) -> Result<bool> {
        if !self.headers.is_empty() {
            // Headers already initialized
            return Ok(false);
        }

        if self.has_headers {
            // Parse headers from this line
            self.headers = self.parse_header_line(line)?;
            Ok(true) // This line was consumed as headers
        } else {
            // Generate column names based on this line
            self.headers = self.generate_column_names(line)?;
            Ok(false) // This line should still be processed as data
        }
    }

    /// Parse the header line and extract column names
    fn parse_header_line(&mut self, line: &str) -> Result<Vec<String>> {
        let mut reader = ReaderBuilder::new()
            .delimiter(self.delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(line.as_bytes());

        if let Some(result) = reader.records().next() {
            let record = result.context("Failed to parse CSV header line")?;
            let headers: Vec<String> = record
                .iter()
                .map(|s| {
                    let raw = s.trim();
                    let mut parts = raw.splitn(2, ':');
                    let field_name = parts.next().unwrap_or("").trim();
                    let field_type = parts.next().map(str::trim);

                    if let Some(type_str) = field_type {
                        if let Some(ftype) = FieldType::from_str(type_str) {
                            if !self.type_map.contains_key(field_name) {
                                self.type_map.insert(field_name.to_string(), ftype);
                            }
                            return field_name.to_string();
                        }
                    }

                    raw.to_string()
                })
                .collect();
            Ok(headers)
        } else {
            Err(anyhow::anyhow!("Empty CSV header line"))
        }
    }

    /// Generate column names for headerless CSV (c1, c2, c3...)
    fn generate_column_names(&self, line: &str) -> Result<Vec<String>> {
        let mut reader = ReaderBuilder::new()
            .delimiter(self.delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(line.as_bytes());

        if let Some(result) = reader.records().next() {
            let record = result.context("Failed to parse CSV data line for column count")?;
            let headers: Vec<String> = (1..=record.len()).map(|i| format!("c{}", i)).collect();
            Ok(headers)
        } else {
            Err(anyhow::anyhow!("Empty CSV data line"))
        }
    }

    /// Build a typed Dynamic value for a field, honoring the type map and strict mode.
    fn build_value(&self, header: &str, field: &str) -> Result<Dynamic> {
        if let Some(field_type) = self.type_map.get(header) {
            // convert_value_to_type handles strict mode internally:
            // - strict=true: returns Err on failure
            // - strict=false: returns Ok(string) on failure
            convert_value_to_type(field, field_type, self.strict).map_err(|e| {
                anyhow::anyhow!("Type conversion failed for field '{}': {}", header, e)
            })
        } else {
            // No type annotation, store as string
            Ok(Dynamic::from(field.to_string()))
        }
    }

    /// Set the field at 0-based position `i`. Columns beyond the known headers
    /// get positional names (c5, c6, ...) — the same convention headerless mode
    /// uses — so ragged rows lose no data. A real header with that exact name
    /// keeps its value; the overflow column is not allowed to clobber it.
    fn set_positional_field(&self, event: &mut Event, i: usize, field: &str) -> Result<()> {
        match self.headers.get(i) {
            Some(header) => {
                let value = self.build_value(header, field)?;
                event.set_field(header.clone(), value);
            }
            None => {
                let name = format!("c{}", i + 1);
                if !self.headers.iter().any(|h| *h == name) {
                    event.set_field(name, Dynamic::from(field.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Account for ragged rows: count them as diagnostics in resilient mode,
    /// reject them under --strict. Rows wider than the header keep their extra
    /// columns as cN fields; narrower rows simply leave the trailing fields
    /// absent (absent, not empty — `field in e` stays meaningful downstream).
    fn check_row_shape(&self, field_count: usize) -> Result<()> {
        let expected = self.headers.len();
        match field_count.cmp(&expected) {
            std::cmp::Ordering::Equal => Ok(()),
            std::cmp::Ordering::Greater => {
                if self.strict {
                    return Err(self.ragged_row_error(field_count));
                }
                crate::stats::stats_add_csv_row_extra_columns(expected + 1);
                Ok(())
            }
            std::cmp::Ordering::Less => {
                if self.strict {
                    return Err(self.ragged_row_error(field_count));
                }
                crate::stats::stats_add_csv_row_missing_columns();
                Ok(())
            }
        }
    }

    /// Strict-mode shape error, naming where the expected width came from so
    /// headerless runs don't leave users guessing about the count's origin.
    fn ragged_row_error(&self, field_count: usize) -> anyhow::Error {
        let origin = if self.has_headers {
            "from header"
        } else {
            "from first line"
        };
        anyhow::anyhow!(
            "Row has {} columns, expected {} ({})",
            field_count,
            self.headers.len(),
            origin
        )
    }

    /// Parse a data line using the initialized headers
    fn parse_data_line(&self, line: &str) -> Result<Event> {
        // Fast path: when the line contains no quote characters, the csv crate's
        // default behavior is byte-for-byte identical to splitting on the delimiter
        // (no trimming, no escape handling). This avoids constructing a fresh
        // csv::Reader — with its 8 KB internal buffer — for every single line.
        if !line.as_bytes().contains(&b'"') {
            let mut event = Event::with_capacity(line.to_string(), self.headers.len());
            let mut field_count = 0;
            for (i, field) in line.split(self.delimiter as char).enumerate() {
                field_count = i + 1;
                self.set_positional_field(&mut event, i, field)?;
            }
            self.check_row_shape(field_count)?;
            if self.auto_timestamp {
                event.extract_timestamp();
            }
            return Ok(event);
        }

        // Slow path: quoted fields require the full csv parser.
        let mut reader = ReaderBuilder::new()
            .delimiter(self.delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(line.as_bytes());

        if let Some(result) = reader.records().next() {
            let record = result.context("Failed to parse CSV data line")?;
            let mut event = Event::with_capacity(line.to_string(), record.len());

            // Map CSV fields to event using local headers
            for (i, field) in record.iter().enumerate() {
                self.set_positional_field(&mut event, i, field)?;
            }
            self.check_row_shape(record.len())?;

            if self.auto_timestamp {
                event.extract_timestamp();
            }
            Ok(event)
        } else {
            Err(anyhow::anyhow!("Empty CSV record"))
        }
    }

    /// Create a special skip event for header lines
    fn create_skip_event(&self, line: &str) -> Event {
        let mut event = Event::default_with_line(line.to_string());
        event.set_field("__skip__".to_string(), Dynamic::from(true));
        event
    }
}

impl EventParser for CsvParser {
    fn parse(&self, line: &str) -> Result<Event> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        // Skip empty lines
        if line.trim().is_empty() {
            return Ok(self.create_skip_event(line));
        }

        // If headers are not initialized, this is sequential mode - use old behavior
        if self.headers.is_empty() {
            return Err(anyhow::anyhow!(
                "CSV parser not properly initialized. This should not happen in normal usage."
            ));
        }

        // Parse data line using pre-initialized headers
        self.parse_data_line(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference splitter: run the real csv crate over a single line and collect
    /// its fields. This is exactly the slow path the fast path must agree with.
    fn csv_crate_fields(line: &str, delimiter: u8) -> Vec<String> {
        let mut reader = ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(line.as_bytes());
        let record = reader
            .records()
            .next()
            .expect("non-empty line")
            .expect("valid record");
        record.iter().map(|s| s.to_string()).collect()
    }

    /// Differential test: for lines containing no quote character, the fast
    /// `str::split` path must produce exactly the same fields as the csv crate.
    #[test]
    fn fast_path_matches_csv_crate_for_unquoted_lines() {
        // Tricky inputs, none containing a double quote.
        let cases = [
            "a,b,c",
            "Alice,25,New York",
            "a,,b",              // empty middle field
            ",a,b",              // leading empty field
            "a,b,",              // trailing empty field
            ",,",                // all empty
            "single",            // no delimiter at all
            " a , b , c ",       // surrounding whitespace must be preserved (Trim::None)
            "a\\,b\\n",          // literal backslashes are not escapes
            "café,naïve,日本語", // multibyte UTF-8 around ASCII delimiters
            "tab\tinside",       // a tab inside a comma-delimited field is just data
            "1,2,3,4,5,6,7,8",   // more fields than headers
        ];

        for &line in &cases {
            let expected = csv_crate_fields(line, b',');

            // Headers wide enough to capture every field the csv crate produced.
            let headers: Vec<String> = (1..=expected.len()).map(|i| format!("c{i}")).collect();
            let parser =
                CsvParser::new_csv_with_headers(headers.clone()).with_auto_timestamp(false);

            let event = parser.parse(line).expect("fast path parse");

            for (i, header) in headers.iter().enumerate() {
                let got = event
                    .fields
                    .get(header.as_str())
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                assert_eq!(
                    got, expected[i],
                    "mismatch on line {line:?} field {header}: fast={got:?} csv={:?}",
                    expected[i]
                );
            }
        }
    }

    /// The same guarantee for the tab dialect.
    #[test]
    fn fast_path_matches_csv_crate_for_unquoted_tsv() {
        let cases = ["a\tb\tc", "a\t\tb", "\ta\tb", " a \t b ", "one"];
        for &line in &cases {
            let expected = csv_crate_fields(line, b'\t');
            let headers: Vec<String> = (1..=expected.len()).map(|i| format!("c{i}")).collect();
            let parser =
                CsvParser::new_tsv_with_headers(headers.clone()).with_auto_timestamp(false);
            let event = parser.parse(line).expect("fast path parse");
            for (i, header) in headers.iter().enumerate() {
                let got = event
                    .fields
                    .get(header.as_str())
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                assert_eq!(got, expected[i], "tsv mismatch on {line:?} field {header}");
            }
        }
    }

    #[test]
    fn test_csv_with_headers() {
        let mut parser = CsvParser::new_csv();

        // Initialize headers from first line
        let header_line = "name,age,city";
        let was_consumed = parser.initialize_headers_from_line(header_line).unwrap();
        assert!(was_consumed); // Header line should be consumed

        // Second line should be data
        let data_result = parser.parse("Alice,25,New York").unwrap();
        assert_eq!(data_result.fields.get("name").unwrap().to_string(), "Alice");
        assert_eq!(data_result.fields.get("age").unwrap().to_string(), "25");
        assert_eq!(
            data_result.fields.get("city").unwrap().to_string(),
            "New York"
        );
    }

    #[test]
    fn test_csv_no_headers() {
        let mut parser = CsvParser::new_csv_no_headers();

        // Initialize column names from first line
        let data_line = "Alice,25,New York";
        let was_consumed = parser.initialize_headers_from_line(data_line).unwrap();
        assert!(!was_consumed); // Data line should not be consumed

        // First line should be data with generated column names
        let data_result = parser.parse("Alice,25,New York").unwrap();
        assert_eq!(data_result.fields.get("c1").unwrap().to_string(), "Alice");
        assert_eq!(data_result.fields.get("c2").unwrap().to_string(), "25");
        assert_eq!(
            data_result.fields.get("c3").unwrap().to_string(),
            "New York"
        );
    }

    #[test]
    fn test_tsv_with_headers() {
        let mut parser = CsvParser::new_tsv();

        // Initialize headers from first line
        let header_line = "name\tage\tcity";
        let was_consumed = parser.initialize_headers_from_line(header_line).unwrap();
        assert!(was_consumed); // Header line should be consumed

        // Second line should be data
        let data_result = parser.parse("Alice\t25\tNew York").unwrap();
        assert_eq!(data_result.fields.get("name").unwrap().to_string(), "Alice");
        assert_eq!(data_result.fields.get("age").unwrap().to_string(), "25");
        assert_eq!(
            data_result.fields.get("city").unwrap().to_string(),
            "New York"
        );
    }

    #[test]
    fn test_csv_with_quotes() {
        let mut parser = CsvParser::new_csv();

        // Initialize headers
        let _ = parser.initialize_headers_from_line("name,message").unwrap();

        // Parse quoted data
        let data_result = parser.parse("\"John Smith\",\"Hello, world!\"").unwrap();
        assert_eq!(
            data_result.fields.get("name").unwrap().to_string(),
            "John Smith"
        );
        assert_eq!(
            data_result.fields.get("message").unwrap().to_string(),
            "Hello, world!"
        );
    }

    #[test]
    fn test_csv_with_escaped_quotes() {
        let mut parser = CsvParser::new_csv();

        // Initialize headers
        let _ = parser.initialize_headers_from_line("name,message").unwrap();

        // Parse data with escaped quotes
        let data_result = parser
            .parse("\"John\",\"He said \"\"hello\"\" to me\"")
            .unwrap();
        assert_eq!(data_result.fields.get("name").unwrap().to_string(), "John");
        assert_eq!(
            data_result.fields.get("message").unwrap().to_string(),
            "He said \"hello\" to me"
        );
    }

    #[test]
    fn test_csv_empty_fields() {
        let mut parser = CsvParser::new_csv();

        // Initialize headers
        let _ = parser
            .initialize_headers_from_line("name,age,city")
            .unwrap();

        // Parse data with empty fields
        let data_result = parser.parse("Alice,,Boston").unwrap();
        assert_eq!(data_result.fields.get("name").unwrap().to_string(), "Alice");
        assert_eq!(data_result.fields.get("age").unwrap().to_string(), "");
        assert_eq!(
            data_result.fields.get("city").unwrap().to_string(),
            "Boston"
        );
    }

    #[test]
    fn test_csv_variable_columns() {
        let mut parser = CsvParser::new_csv_no_headers();

        // Initialize column names from first line
        let first_line = "Alice,25";
        let _ = parser.initialize_headers_from_line(first_line).unwrap();

        // Parse data with initial column count
        let data_result1 = parser.parse("Alice,25").unwrap();
        assert_eq!(data_result1.fields.get("c1").unwrap().to_string(), "Alice");
        assert_eq!(data_result1.fields.get("c2").unwrap().to_string(), "25");
        assert!(data_result1.fields.get("c3").is_none());

        // Second line with more columns: overflow keeps positional names
        let data_result2 = parser.parse("Bob,30,Engineer").unwrap();
        assert_eq!(data_result2.fields.get("c1").unwrap().to_string(), "Bob");
        assert_eq!(data_result2.fields.get("c2").unwrap().to_string(), "30");
        assert_eq!(
            data_result2.fields.get("c3").unwrap().to_string(),
            "Engineer"
        );

        // Shorter line: trailing fields are absent, not empty
        let data_result3 = parser.parse("Carol").unwrap();
        assert_eq!(data_result3.fields.get("c1").unwrap().to_string(), "Carol");
        assert!(data_result3.fields.get("c2").is_none());
    }

    #[test]
    fn test_csv_headered_overflow_gets_positional_names() {
        let mut parser = CsvParser::new_csv();
        let _ = parser
            .initialize_headers_from_line("name,age,city")
            .unwrap();

        let event = parser.parse("Alice,25,Boston,extra1,extra2").unwrap();
        assert_eq!(event.fields.get("name").unwrap().to_string(), "Alice");
        assert_eq!(event.fields.get("city").unwrap().to_string(), "Boston");
        assert_eq!(event.fields.get("c4").unwrap().to_string(), "extra1");
        assert_eq!(event.fields.get("c5").unwrap().to_string(), "extra2");
    }

    #[test]
    fn test_csv_overflow_quoted_slow_path() {
        let mut parser = CsvParser::new_csv();
        let _ = parser.initialize_headers_from_line("name,msg").unwrap();

        let event = parser.parse("\"Alice\",\"hello, world\",overflow").unwrap();
        assert_eq!(event.fields.get("name").unwrap().to_string(), "Alice");
        assert_eq!(event.fields.get("msg").unwrap().to_string(), "hello, world");
        assert_eq!(event.fields.get("c3").unwrap().to_string(), "overflow");
    }

    #[test]
    fn test_csv_overflow_does_not_clobber_real_header() {
        // A header literally named "c3" wins over an overflow column at
        // position 3; the colliding overflow value is dropped.
        let mut parser = CsvParser::new_csv();
        let _ = parser.initialize_headers_from_line("a,c3").unwrap();

        let event = parser.parse("x,y,z").unwrap();
        assert_eq!(event.fields.get("a").unwrap().to_string(), "x");
        assert_eq!(event.fields.get("c3").unwrap().to_string(), "y");
    }

    #[test]
    fn test_csv_strict_rejects_ragged_rows() {
        let mut parser = CsvParser::new_csv();
        let _ = parser
            .initialize_headers_from_line("name,age,city")
            .unwrap();
        let parser = parser.with_strict(true);

        // Matching width still parses
        assert!(parser.parse("Alice,25,Boston").is_ok());

        // Too many columns: hard error (fast path and quoted slow path)
        let err = parser.parse("Bob,30,Boston,extra").unwrap_err();
        assert!(
            err.to_string().contains("expected 3 (from header)"),
            "{}",
            err
        );
        assert!(parser.parse("\"Bob\",30,Boston,extra").is_err());

        // Too few columns: hard error
        let err = parser.parse("Carol,41").unwrap_err();
        assert!(
            err.to_string().contains("expected 3 (from header)"),
            "{}",
            err
        );
    }

    #[test]
    fn test_csv_strict_headerless_error_names_first_line_origin() {
        let mut parser = CsvParser::new_csv_no_headers();
        let _ = parser.initialize_headers_from_line("a,b").unwrap();
        let parser = parser.with_strict(true);

        let err = parser.parse("1,2,3").unwrap_err();
        assert!(
            err.to_string().contains("expected 2 (from first line)"),
            "{}",
            err
        );
    }

    #[test]
    fn test_csv_header_type_annotations() {
        let mut parser = CsvParser::new_csv();

        let header_line = "name,status:int,bytes:int";
        let was_consumed = parser.initialize_headers_from_line(header_line).unwrap();
        assert!(was_consumed);

        let data_result = parser.parse("Alice,200,1234").unwrap();
        assert_eq!(data_result.fields.get("name").unwrap().to_string(), "Alice");
        assert_eq!(
            data_result.fields.get("status").unwrap().as_int().unwrap(),
            200
        );
        assert_eq!(
            data_result.fields.get("bytes").unwrap().as_int().unwrap(),
            1234
        );
        assert!(data_result.fields.get("status:int").is_none());
    }
}
