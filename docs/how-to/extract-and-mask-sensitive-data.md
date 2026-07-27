# Sanitize Logs Before Sharing

Remove personally identifiable information (PII), secrets, and other sensitive values from log streams so they can be reviewed or archived safely.

## Typical Use Cases
- Preparing logs for customer support, vendors, or public issue trackers.
- Creating test fixtures or reproductions without exposing production data.
- Complying with privacy regulations before storing logs long term.

## Before You Start
- Examples use `examples/security_audit.jsonl`, and `examples/email_logs.log` from Step 4 on, where the sensitive values sit inside message text rather than in fields of their own; swap in your own files.
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
  -e 'e = e.drop(["token", "hash", "reason"])' \
  -e 'e = e.keep(["timestamp", "event", "user", "ip"])' \
  -F json
```

```
{"timestamp":"2024-01-15T10:00:00Z","event":"login","user":"alice","ip":"192.168.1.10"}
```

Tips:

- `drop([...])` removes a batch of known-sensitive fields.
- `keep([...])` whitelists the final output shape so unexpected fields do not leak through.
- `--exclude-keys field1,field2` works when the data is already flat.

## Step 3: Mask Direct Identifiers
Use helper functions for IPs and structured values.

```bash
kelora -j examples/security_audit.jsonl \
  -e 'if e.contains("ip") { e.ip = e.ip.mask_ip(2) }' \
  -e 'if e.contains("email") {
        let parts = e.email.parse_email();
        e.email_domain = parts.domain;
        e.email = ()
      }' \
  -F json
```

- `mask_ip()` anonymises IPv4 and IPv6 addresses by zeroing the masked suffix while preserving network information.
- Extract domains or other aggregates before dropping the original field if analysts still need grouped statistics.
- The `email` branch fires only where that field exists — `security_audit.jsonl` keeps no addresses, so on this sample only the IP masking shows up. Guarding with `e.contains(...)` is what keeps the other branch quiet instead of erroring.

## Step 4: Scrub Free-Form Text
Sanitise values embedded in log messages using extraction and replacement. This step switches to `examples/email_logs.log`, whose messages carry addresses inline — `security_audit.jsonl` holds its values in discrete fields, which Step 3 already covers.

```bash
# Extract email domains before redacting the addresses themselves
kelora -f 'cols:timestamp level *message' examples/email_logs.log \
  -e 'if e.message.extract_email() != "" {
        e.email_domain = e.message.extract_email().after("@");
        e.message = e.message.replace_regex(#"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#, "[EMAIL]")
      }' \
  -e 'e.message = e.message.replace_regex(#"(?i)api[_-]?key=\w+"#, "api_key=[REDACTED]")' \
  -F json
```

```
{"timestamp":"2025-01-15T10:23:45Z","level":"INFO","message":"Email sent from [EMAIL] to [EMAIL] subject=\"Welcome\"","email_domain":"example.com"}
```

Use `extract_email()`, `extract_ip()`, or `extract_url()` to identify sensitive data in unstructured text before masking. Consider building a short library of patterns that match your organisation's identifiers (customer numbers, ticket IDs, etc.).

- Redaction needs `replace_regex()`. Plain `replace()` matches a literal substring, so handing it a pattern leaves every address in place.
- The second `-e` is a second pattern rather than a second step — this sample carries no API keys, so it changes nothing here.

## Step 5: Validate the Result
Check that sensitive patterns no longer appear. Use `--assert` for this, not `--filter`: the assertion's exit code is the gate (`0` when every event passes, `1` when one fails), while `--filter` exits `0` whether or not anything matched, so a `--filter` gate always takes the same branch.

```bash
kelora -f 'cols:timestamp level *message' examples/email_logs.log \
  -e 'e.message = e.message.replace_regex(#"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#, "[EMAIL]")' \
  -J > sanitized.json

kelora -j sanitized.json \
  -q \
  --assert '!(e.message ?? "").contains("@")' \
  --assert '!(e.message ?? "").matches(#"\b\d{3}-\d{2}-\d{4}\b"#)' \
  && echo "Sanitisation checks passed" \
  || echo "WARNING: potential PII found"
```

On this sample the check fires, which is the point of running it:

```
kelora: assert failed: !(e.message ?? "").contains("@")
  line 6: timestamp='2025-01-15T10:25:45Z' level='ERROR' message='Failed to send email to broken@email reason="Invalid address format"'
kelora: Processing completed with 1 assertion failure
kelora: 1 assertion failure
WARNING: potential PII found
```

`broken@email` has no top-level domain, so neither the email pattern nor `extract_email()` recognises it as an address — exactly the kind of gap a validation pass exists to catch.

- Write the check as `(e.message ?? "")`, not `e.message`: a bare field access raises a missing-field error on any event that carries no message, which fails the gate for the wrong reason.
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
    -e 'if e.contains("ip") { track_sum("ip_fields", 1) }' \
    -e 'if e.contains("token") { track_sum("token_fields", 1) }' \
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
