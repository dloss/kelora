# Pseudonymize Identifiers for Analytics

Replace user identifiers with consistent, non-reversible tokens so you can analyse behaviour without exposing raw PII.

## Why Use This Approach
- Analysts need to count or group by user, tenant, or device while respecting privacy.
- Logs must be exported outside the production boundary (vendors, BI tools).
- You want deterministic transformations that do not require a central mapping database.

## Before You Start
- Set the `KELORA_SECRET` environment variable to a strong, random string. Kelora derives pseudonyms from this secret.
- Plan domain separation keys (`"users"`, `"sessions"`, `"tenants"`). Different domains prevent correlating identities across datasets.
- Decide how you will rotate the secret. Rotating changes all generated tokens; coordinate with downstream consumers.

## Step 1: Configure the Secret
Export the secret in the shell or set it within your scheduler/service definition.

```bash
export KELORA_SECRET="$(openssl rand -hex 32)"
```

- Keep the value in a secret manager or environment configuration, not in source control.
- All Kelora invocations must share the same secret to maintain consistency.

## Step 2: Generate Pseudonyms for Primary Identifiers
Use the `pseudonym()` function to create consistent tokens per domain.

```bash
kelora -j examples/security_audit.jsonl \
  -e 'e.user_id = pseudonym(e.user_email, "users")' \
  -e 'e.session_id = pseudonym(e.session_token, "sessions")' \
  -e 'e.user_email = (); e.session_token = ()' \
  -F json
```

- Passing different domains (`"users"`, `"sessions"`) ensures tokens cannot be matched across contexts.
- Drop the original fields immediately after generating pseudonyms.

## Step 3: Preserve Useful Context
Extract secondary attributes (domains, regions) before removing raw data so analysts retain dimensional information.

```bash
kelora -j examples/security_audit.jsonl \
  -e 'let email = e.user_email.parse_email();
        e.user_domain = email.domain;
        e.user_id = pseudonym(e.user_email, "users");
        e.user_email = ()' \
  -F json
```

- Combine with masking (`mask_ip`) when you also need approximate locations or network groupings.
- Track counts of pseudonyms (`track_unique`) to estimate active user bases without storing clear text identifiers.

## Step 4: Compare Against Hashing
When you only need grouping (and not reversibility), hashing may suffice. Benchmark both approaches so downstream tools understand collision risks.

```bash
kelora -j app.log \
  -e 'e.user_hash = e.user_id.hash("xxh3")' \
  -e 'e.user_anon = pseudonym(e.user_id, "users")' \
  -e 'track_unique("hashes", e.user_hash)' \
  -e 'track_unique("pseudonyms", e.user_anon)' \
  --metrics
```

- `hash("xxh3")` is fast but not secret; use `pseudonym()` whenever anonymity guarantees matter.
- Cryptographic hashes (`sha256`, `blake3`) avoid collisions but still leak the same input if the attacker knows it. Pseudonyms resist this by requiring the secret.

## Step 5: Validate Consistency
Confirm that the same inputs produce identical pseudonyms across runs and that expected fields are gone.

```bash
kelora -j pseudonymized.json \
  -q \
  --filter '!e.contains("user_email") && !e.contains("session_token")' \
  --stats

kelora -j examples/security_audit.jsonl \
  -e 'let token = pseudonym(e.user_email, "users");' \
  -e 'track_unique("tokens", token)' \
  --metrics
```

- Use small fixture files in tests to guard against accidental changes to the pseudonym rules.
- When rotating secrets, run both the old and new pipeline on a sample and confirm the change is expected.

## Variations
- **Tenant isolation**
  ```bash
  kelora -j multi_tenant.log \
    -e 'let tenant = e.tenant_name;
          e.tenant_id = pseudonym(tenant, "tenants");
          e.user_id = pseudonym(e.user_email, tenant);
          e.tenant_name = (); e.user_email = ()' \
    -F json
  ```
  Tokens cannot be correlated between tenants while remaining consistent inside each tenant.

- **Selective reversibility**
  - Keep a secure mapping of `(original_id, pseudonym)` when legal/compliance teams need an escalation path.
  - Store the mapping in an encrypted datastore; never in the output log files themselves.
- **Secret rotation checklist**
  1. Deploy new secret value.
  2. Run both old and new pipelines in parallel for validation.
  3. Notify downstream teams that historical comparisons may require dual ingestion.

## Operational Tips
- Do not log the secret or intermediate values in `--verbose` output.
- Combine with [Sanitize Logs Before Sharing](extract-and-mask-sensitive-data.md) to remove residual text-based identifiers.
- Review legal requirements around pseudonymisation; some jurisdictions treat pseudonyms as personal data if the secret is accessible.

## See Also
- [Sanitize Logs Before Sharing](extract-and-mask-sensitive-data.md) for masking and redaction patterns.
- [Prepare CSV Exports for Analytics](process-csv-data.md) to package pseudonymised data for downstream tools.
- [Concept: Configuration System](../concepts/configuration-system.md) if you want to store secrets in Kelora config files rather than shell exports.
