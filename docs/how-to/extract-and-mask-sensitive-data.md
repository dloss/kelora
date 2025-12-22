# Sanitize Logs Before Sharing

Remove personally identifiable information (PII), secrets, and other sensitive values from log streams so they can be reviewed or archived safely.

## Typical Use Cases
- Preparing logs for customer support, vendors, or public issue trackers.
- Creating test fixtures or reproductions without exposing production data.
- Complying with privacy regulations before storing logs long term.

## Before You Start
- Examples use `examples/security_audit.jsonl`; swap in your own files.
- Decide what must be removed (e.g., IPs, emails, tokens, stack traces). Document these requirements before implementation.
- Sanitisation often reduces context. Keep an untampered copy in a protected location for security responders.

## Step 1: Catalogue Sensitive Fields
Inspect a small sample to confirm field names and formats.

```bash
kelora -j examples/security_audit.jsonl --take 5
```

List the columns that require masking (`user_email`, `ip_address`, `token`, etc.) and note whether the data appears as discrete fields or inside messages.

## Step 2: Drop or Whitelist Fields
Remove fields you definitely do not need, or rebuild each event with only the essentials.

```bash
kelora -j examples/security_audit.jsonl \
  -e 'e.password = (); e.token = (); e.session = ()' \
  -e 'let keep = #{timestamp: e.timestamp, service: e.service, message: e.message}; e = keep' \
  -F json
```

Tips:

- Assigning `()` deletes a field.
- Rebuilding the event (as above) ensures no unexpected data leaks.
- `--exclude-keys field1,field2` works when the data is already flat.

## Step 3: Mask Direct Identifiers
Use helper functions for IPs and structured values.

```bash
kelora -j examples/security_audit.jsonl \
  -e 'if e.contains("ip_address") { e.ip_address = e.ip_address.mask_ip(2) }' \
  -e 'if e.contains("email") {
        let parts = e.email.parse_email();
        e.email_domain = parts.domain;
        e.email = ()
      }' \
  -F json
```

- `mask_ip()` anonymises IPv4 and IPv6 addresses while preserving network information.
- Extract domains or other aggregates before dropping the original field if analysts still need grouped statistics.

## Step 4: Scrub Free-Form Text
Sanitise values embedded in log messages using extraction and replacement.

```bash
# Extract email addresses before redacting
kelora -j examples/security_audit.jsonl \
  -e 'if e.message.extract_email() != "" {
        e.email_domain = e.message.extract_email().after("@");
        e.message = e.message.replace(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}", "[EMAIL]")
      }' \
  -e 'e.message = e.message.replace(r"(?i)api[_-]?key=\\w+", "api_key=[REDACTED]")' \
  -F json
```

Use `extract_email()`, `extract_ip()`, or `extract_url()` to identify sensitive data in unstructured text before masking. Consider building a short library of patterns that match your organisation's identifiers (customer numbers, ticket IDs, etc.).

## Step 5: Validate the Result
Run automated checks to ensure sensitive patterns no longer appear.

```bash
kelora -j sanitized.json \
  -q \
  --filter 'e.message.matches("\\b\\d{3}-\\d{2}-\\d{4}\\b") || e.message.matches("@")' \
  && echo "WARNING: potential PII found" \
  || echo "Sanitisation checks passed"
```

- Build explicit checks for each high-risk pattern (credit cards, SSNs, phone numbers).
- Add `--stats` when sharing the data so recipients can see how many events were processed and whether any parsing errors occurred.

## Variations
- **Automated daily scrub**
  ```bash
  export OUTPUT=/secure/sanitized-$(date +%Y-%m-%d).json
  kelora -j /var/log/app/app-$(date +%Y-%m-%d).log \
    -e 'e.ip = e.ip.mask_ip(2)' \
    -e 'e.email = ()' \
    -e 'e.card = ()' \
    -J > "$OUTPUT"
  ```

- **Redact stack traces for non-engineers**
  ```bash
  kelora -j app.log \
    -e 'if e.level != "DEBUG" { e.stack_trace = () }' \
    -F json
  ```

- **Metrics to confirm coverage**
  ```bash
  kelora -j app.log \
    -e 'if e.contains("ip") { track_count("ip_fields") }' \
    -e 'if e.contains("token") { track_count("token_fields") }' \
    --metrics
  ```

## Good Practices
- Keep the sanitisation script in version control and review it whenever log formats change.
- Run sanity checks on both raw and sanitised logs to confirm volume and error counts match.
- Document what was removed so downstream teams know whether they must contact the security team for raw data.

## See Also
- [Pseudonymize Identifiers for Analytics](pseudonymize-identifiers-for-analytics.md) when you need consistent but anonymised identifiers.
- [Prepare CSV Exports for Analytics](process-csv-data.md) to share structured subsets.
- [Design Streaming Alerts](build-streaming-alerts.md) for live monitoring of sanitised pipelines.
