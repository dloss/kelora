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

## Drop sensitive fields

`-K token,password` hides fields from the output; `e.token = ()` in `--exec`
removes them before later stages and summaries see them.
