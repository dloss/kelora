# Security and Privacy

## Failed SSH logins by IP and user

```bash exec="on" source="above" result="ansi"
kelora examples/sshd_auth.log --input-year 2024 \
  --filter 'e.msg.starts_with("Failed password")' \
  -e 'e.ip = e.msg.extract_ip(); e.user = e.msg.extract_regex("for (?:invalid user )?(\\S+)", 1)' \
  --freq ip --freq user
```

## Read JWT claims

`parse_jwt()` decodes a token without verifying the signature — for
debugging, not for trusting:

```bash exec="on" source="above" result="ansi"
kelora examples/auth-logs.jsonl \
  -e 'let t = e.token.parse_jwt(); e.sub = t.claims.sub; e.role = t.claims.role; e.expires = t.expires_at' \
  -k timestamp,sub,role,expires
```

## Requests from one network

```bash exec="on" source="above" result="ansi"
kelora examples/web_access.log --filter 'e.ip.is_in_cidr("198.51.100.0/24")' -k ip,path,status
```

## Mask addresses inside free text

```bash exec="on" source="above" result="ansi"
kelora examples/email_logs.log -f 'cols:ts level *msg' -e 'e.msg = e.msg.normalized(["email", "ipv4"])' -n 3
```

## Consistent pseudonyms

The same input gives the same alias as long as `KELORA_SECRET` stays the
same, so you can still count, join, and follow a user across files:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/user-data.jsonl \
  -e 'e.user = pseudonym(e.email, "user"); e.email = (); e.ip = e.ip.mask_ip(1)' -k user,action,ip
```

## Pseudonymize IP addresses

`--pseudonymize ip` replaces every address in every field, inside messages
too, with a keyed token. It runs right after parsing, so filters, `--freq` and
`--discover` see only tokens; the same address keeps the same token while
`KELORA_SECRET` stays the same:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/sshd_auth.log --input-year 2024 \
  --pseudonymize ip -k msg -n 3
```

Add `email` to replace email addresses the same way:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/user-data.jsonl --pseudonymize ip,email -n 2
```

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

Prefixes are exactly what an attacker can use against it; for data that leaves your control, prefer `--pseudonymize ip` ([trade-offs](../reference/functions.md#textpseudonymize_ipsmode-mappseudonymize_ipsmode)). For one field only, use `pseudonymize_ips()` in `--exec`.

## Drop sensitive fields

`-K token,password` hides fields from the output; `e.token = ()` in `--exec`
removes them before later stages and summaries see them.
