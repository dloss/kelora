# Exports and Samples

## CSV for a spreadsheet

```bash exec="on" source="above" result="ansi"
kelora examples/api_logs.jsonl -l error -k timestamp,service,status,message -F csv
```

Add `-o errors.csv` to write a file. To rename a column, rename the field:
`-e 'e.rename_field("ts", "timestamp")'`.

## Share failed requests with a vendor

Non-2xx only, users replaced by stable pseudonyms, client IPs cut to /16,
query strings (which can carry IDs) removed:

```bash exec="on" source="above" result="ansi"
KELORA_SECRET=change-me kelora examples/web_access.log \
  --filter 'e.status < 200 || e.status >= 300' \
  -e 'if e.has("user") { e.user = pseudonym(e.user, "user") }
      e.ip = e.ip.mask_ip(2); e.path = e.path.split("?")[0]' \
  -k ts,path,status,user,ip -F csv
```

## A reproducible 10 % sample

Hash a stable field, so the same requests are chosen every time:

```bash exec="on" source="above" result="ansi"
kelora examples/web_access_large.log.gz --filter 'e.ip.bucket() % 10 == 0' -J -n 2
```

## One row per array element

`-e 'emit_each(e.orders, #{batch_id: e.batch_id})'` replaces each event with one
event per element of `orders`, keeping the batch ID. Add `-F csv -k …` for a
table. [Transform with Scripts](../guide/scripting.md#one-event-per-array-element)
