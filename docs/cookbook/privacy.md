# Privacy and Sanitization

Strip personal data and secrets from a log before you share it, while keeping
it useful. The recipes go from blunt to fine: drop fields, cut values down,
replace patterns with placeholders, swap identifiers for stable pseudonyms,
then check what's left. Most use `checkout_pii.jsonl`, a shop log with
emails, IPs, card and phone numbers, and tokens in URLs and headers.

## Find what needs hiding

`--discover` lists every field, nested ones included, with examples:

```bash exec="on" source="above" result="ansi"
kelora examples/checkout_pii.jsonl --discover
```

Free text (`msg` here) needs a look of its own: personal data hides in
messages, URLs and headers as often as in named fields.

## Keep only the fields you need

List the fields to keep, not the ones to drop. A field added to the log
next month then stays out until you add it:

```bash exec="on" source="above" result="ansi"
kelora examples/checkout_pii.jsonl -k timestamp,level,service,msg -n 3
```

`-K session,user` does the opposite. Both only shape the output: filters,
`--freq` and metrics still see every field. To remove a field before they
run, do it in a script: `e = e.keep(["timestamp", "level", "service", "msg"])`,
`e = e.drop(["session"])` or `e.session = ()`.

## Remove a nested field

`-k` and `-K` act on top-level fields only. Remove a nested key with
`remove_path()`; events without it are left alone:

```bash exec="on" source="above" result="ansi"
kelora examples/checkout_pii.jsonl -e 'e.remove_path("request.headers.authorization")' -k request -n 2
```

`e.request.headers.authorization = ()` would leave `"authorization": null`.

## Cut values down

Keep the part you need: the network instead of the address, the email
domain instead of the person, the path without its query string (which often
carries tokens):

```bash exec="on" source="above" result="ansi"
kelora examples/checkout_pii.jsonl \
  -e 'e.client_ip = e.client_ip.mask_ip(2);
      if e.has("user") { e.user = e.user.parse_email().domain }
      e.path = e.request.path.split("?")[0];' \
  -k client_ip,user,path
```

`mask_ip(n)` zeroes the last *n* octets (IPv6: groups). `shorten(8, "")`
keeps the first 8 characters of a value.

## Fail closed

If a script fails on an event, kelora outputs the event as it was before the
script ran, with the data you meant to hide, and warns on stderr. Here the
webhook event has no `user`:

```bash exec="on" source="above" result="ansi"
kelora examples/checkout_pii.jsonl \
  -e 'e.client_ip = e.client_ip.mask_ip(2); e.user = e.user.parse_email().domain' \
  -k client_ip -n 7
```

`10.0.4.12` slipped through. When sanitizing, add `--strict`: the first error
stops the run with exit code 1, before the event is written:

```bash exec="on" source="above" result="ansi" returncode="1"
kelora examples/checkout_pii.jsonl --strict \
  -e 'e.client_ip = e.client_ip.mask_ip(2); e.user = e.user.parse_email().domain' \
  -k client_ip -n 7
```

Then guard optional fields with `e.has(...)`, as in the recipe above.

## Replace personal data in free text

`normalized()` replaces addresses, IDs and the like inside text with
placeholders. Card, phone and US social security numbers are off by default;
name them:

```bash exec="on" source="above" result="ansi"
kelora examples/checkout_pii.jsonl \
  -e 'e.msg = e.msg.normalized(["email", "ipv4", "ipv6", "credit_card", "phone", "ssn"])' \
  -k service,msg
```

Card numbers must pass the Luhn check, so other long numbers stay. Called on the
event, `e = e.normalized([...])` rewrites every string field, nested ones
included. The [full pattern list](../reference/functions.md#textnormalizedpatterns-mapnormalizedpatterns)
also has `uuid`, `url`, `mac`, `path` and hashes.

## Redact secrets with a regex

Passwords and API keys have no fixed shape, but they usually follow a
known name:

```bash exec="on" source="above" result="ansi"
kelora examples/checkout_pii.jsonl \
  -e 'e.msg = e.msg.replace_regex("(password|api_key|token)=[^&\\s]+", "${1}=***")' \
  -k msg --filter 'e.msg.contains("***")'
```

## Consistent pseudonyms

`pseudonym()` replaces a value with a keyed alias. The same input gives the
same alias as long as `KELORA_SECRET` stays the same, so you can still count,
join, and follow a user across files:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/user-data.jsonl \
  -e 'e.user = pseudonym(e.email, "user"); e.email = (); e.ip = e.ip.mask_ip(1)' -k user,action,ip
```

The second argument is a domain: the same value in two domains gets unrelated
aliases. Don't use `hash()` for this: it's unkeyed, so anyone can hash a
list of candidate emails and match them.

## Pseudonymize every IP and email address

`--pseudonymize ip,email` replaces every address in every field, nested values
and messages included, with a keyed token. Card numbers are untouched:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/checkout_pii.jsonl \
  --pseudonymize ip,email -k user,client_ip,msg -n 3
```

It runs right after parsing, so filters, `--freq` and `--discover` see only
tokens. `--stats` counts the replacements (`Addresses pseudonymized: 9 IP, 11 email`).
`extract_ip()` returns the tokens, so you can still count failed logins per
source address without seeing one:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/sshd_auth.log --input-year 2024 \
  --pseudonymize ip --filter 'e.msg.starts_with("Failed password")' \
  -e 'e.src = e.msg.extract_ip()' --freq src
```

A filter never sees a real address either. To look for a known one,
pseudonymize the literal the same way:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/sshd_auth.log --input-year 2024 \
  --pseudonymize ip --filter 'e.msg.contains("198.51.100.23".pseudonymize_ips())' -k msg
```

For one field only, use `pseudonymize_ips()` or `pseudonymize_emails()` in
`--exec`.

## Find the address behind a token

Tokens can't be reversed, but whoever holds the secret and the original log can
look one up, say after a recipient reports `<ip:wuukJmb_X0e9Dmt1FaY0h3SU>`:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/sshd_auth.log --input-year 2024 \
  --filter 'e.msg.pseudonymize_ips().contains("<ip:wuukJmb_X0e9Dmt1FaY0h3SU>")' -k msg -n 1
```

## Pseudonymize IPs but keep subnets

`--pseudonymize ip-prefix` replaces each address with another of the same
family; addresses that share a prefix still share one afterwards (`10.0.5.23`
and `10.0.5.24` stay in one /24):

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/user-data.jsonl \
  --pseudonymize ip-prefix -k user_id,ip
```

Pseudonymize a network the same way to filter by it:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/user-data.jsonl --pseudonymize ip-prefix \
  --filter 'e.ip.is_in_cidr("10.0.0.0/8".pseudonymize_ips("prefix"))' -k user_id,ip
```

Prefixes are exactly what an attacker can use against it; for data that leaves
your control, prefer `--pseudonymize ip`
([trade-offs](../reference/functions.md#textpseudonymize_ipsmode-mappseudonymize_ipsmode)).

## Keep the secret out of shell history

Use one long random secret per recipient, so two recipients can't link their
copies, and pass it as a file:

```bash
openssl rand -base64 24 > vendor-a.key
KELORA_SECRET_FILE=vendor-a.key kelora app.jsonl --pseudonymize ip,email -F json -o for-vendor-a.jsonl
```

Without a secret, kelora uses a random one and the pseudonyms change on every
run.

## Sanitize a text log and keep its format

Read the file as plain lines and print them back unchanged, apart from
the replacements:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/sshd_auth.log -f line --pseudonymize ip -k line -b -n 3
```

Add `-e 'e.line = e.line.normalized([...])'` or `replace_regex()` for other
data.

## Check what's left

Run the sanitized output through kelora again and count which kinds of
personal data remain:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/checkout_pii.jsonl --pseudonymize ip,email -F json \
  | kelora -j -m -e 'let t = e.to_json();
      for p in ["email", "ipv4", "ipv6", "credit_card", "phone", "ssn"] {
        if t.normalized(p) != t { track_freq("left", p) }
      }'
```

The pseudonymized addresses are gone; card, phone and SSN numbers are not.
Patterns find what has a shape. Names, street addresses and free-form
secrets don't, so read a sample as well.

## All together

Fields chosen, secrets and numbers redacted, addresses pseudonymized, query
strings cut, stopping on any error. Add `-o shared.jsonl` to write a file:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/checkout_pii.jsonl --strict \
  --pseudonymize ip,email \
  -e 'e.path = e.request.path.split("?")[0];
      e.msg = e.msg
        .replace_regex("(password|api_key|token)=[^&\\s]+", "${1}=***")
        .normalized(["credit_card", "phone", "ssn"]);' \
  -k timestamp,level,service,user,client_ip,path,msg -F json -n 4
```
