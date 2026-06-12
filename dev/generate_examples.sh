#!/usr/bin/env bash
# Generate example log files for Kelora demonstrations and testing
# Usage: ./dev/generate_examples.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="examples"

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "${BLUE}🔹${NC} $*"
}

success() {
    echo -e "${GREEN}✓${NC} $*"
}

# Create examples directory if it doesn't exist
log "Setting up examples directory..."
mkdir -p "$EXAMPLES_DIR"

################################################################################
# BASIC FORMAT COVERAGE (8 files)
################################################################################

log "Generating basic format coverage files..."

# 1. simple_json.jsonl - Basic JSON logs, 20 lines, mixed levels
cat > "$EXAMPLES_DIR/simple_json.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","level":"INFO","service":"api","message":"Application started","version":"1.2.3"}
{"timestamp":"2024-01-15T10:00:05Z","level":"DEBUG","service":"api","message":"Loading configuration","config_file":"/etc/app/config.yml"}
{"timestamp":"2024-01-15T10:00:10Z","level":"INFO","service":"database","message":"Connection pool initialized","max_connections":50}
{"timestamp":"2024-01-15T10:01:00Z","level":"WARN","service":"api","message":"High memory usage detected","memory_percent":85}
{"timestamp":"2024-01-15T10:01:30Z","level":"ERROR","service":"database","message":"Query timeout","query":"SELECT * FROM users","duration_ms":5000}
{"timestamp":"2024-01-15T10:02:00Z","level":"INFO","service":"api","message":"Request received","method":"GET","path":"/api/users","user_id":123}
{"timestamp":"2024-01-15T10:02:15Z","level":"DEBUG","service":"cache","message":"Cache hit","key":"user:123","ttl":3600}
{"timestamp":"2024-01-15T10:02:30Z","level":"INFO","service":"api","message":"Response sent","status":200,"duration_ms":45}
{"timestamp":"2024-01-15T10:03:00Z","level":"WARN","service":"auth","message":"Failed login attempt","username":"admin","ip":"192.168.1.100"}
{"timestamp":"2024-01-15T10:03:30Z","level":"ERROR","service":"auth","message":"Account locked","username":"admin","attempts":5}
{"timestamp":"2024-01-15T10:04:00Z","level":"INFO","service":"scheduler","message":"Cron job started","job":"backup","schedule":"0 2 * * *"}
{"timestamp":"2024-01-15T10:05:00Z","level":"DEBUG","service":"scheduler","message":"Running backup script","script":"/usr/local/bin/backup.sh"}
{"timestamp":"2024-01-15T10:10:00Z","level":"INFO","service":"scheduler","message":"Backup completed","size_mb":1024,"duration_ms":300000}
{"timestamp":"2024-01-15T10:15:00Z","level":"CRITICAL","service":"disk","message":"Disk space critical","partition":"/var","free_gb":0.5}
{"timestamp":"2024-01-15T10:16:00Z","level":"ERROR","service":"api","message":"Service unavailable","reason":"disk space"}
{"timestamp":"2024-01-15T10:17:00Z","level":"WARN","service":"monitoring","message":"Alert sent","channel":"slack","severity":"high"}
{"timestamp":"2024-01-15T10:20:00Z","level":"INFO","service":"admin","message":"Disk cleanup initiated","target":"/var/log"}
{"timestamp":"2024-01-15T10:25:00Z","level":"INFO","service":"admin","message":"Cleanup completed","freed_gb":10}
{"timestamp":"2024-01-15T10:26:00Z","level":"INFO","service":"api","message":"Service resumed","downtime_seconds":600}
{"timestamp":"2024-01-15T10:30:00Z","level":"DEBUG","service":"health","message":"Health check passed","endpoints":["api","database","cache"]}
EOF
success "Created simple_json.jsonl (20 events, mixed levels)"

# 2. simple_line.log - Plain text logs, 15 lines for basic filtering
cat > "$EXAMPLES_DIR/simple_line.log" <<'EOF'
2024-01-15 10:00:00 [INFO] Server starting on port 8080
2024-01-15 10:00:01 [DEBUG] Loading plugins from /etc/plugins
2024-01-15 10:00:02 [INFO] Database connection established
2024-01-15 10:00:05 [WARN] Configuration file missing, using defaults
2024-01-15 10:01:00 [ERROR] Failed to connect to Redis: connection refused
2024-01-15 10:01:01 [INFO] Falling back to in-memory cache
2024-01-15 10:02:00 [INFO] HTTP server ready
2024-01-15 10:02:30 [DEBUG] Processing GET /health
2024-01-15 10:03:00 [WARN] Rate limit exceeded for IP 10.0.0.5
2024-01-15 10:04:00 [ERROR] Unhandled exception in request handler
2024-01-15 10:04:01 [DEBUG] Stack trace: at handleRequest (app.js:42)
2024-01-15 10:05:00 [CRITICAL] Out of memory, terminating
2024-01-15 10:05:01 [INFO] Shutdown signal received
2024-01-15 10:05:02 [INFO] Closing database connections
2024-01-15 10:05:03 [INFO] Server stopped gracefully
EOF
success "Created simple_line.log (15 lines, plain text)"

# 3. simple_csv.csv - CSV with headers, 25 rows with status/bytes/duration
cat > "$EXAMPLES_DIR/simple_csv.csv" <<'EOF'
timestamp,method,path,status,bytes,duration_ms
2024-01-15T10:00:00Z,GET,/,200,1234,45
2024-01-15T10:00:05Z,GET,/api/users,200,5678,120
2024-01-15T10:00:10Z,POST,/api/login,200,234,67
2024-01-15T10:00:15Z,GET,/api/users/123,200,890,34
2024-01-15T10:00:20Z,PUT,/api/users/123,200,456,89
2024-01-15T10:00:25Z,GET,/static/logo.png,200,12345,12
2024-01-15T10:00:30Z,GET,/api/posts,200,9876,156
2024-01-15T10:00:35Z,GET,/favicon.ico,404,0,5
2024-01-15T10:00:40Z,POST,/api/posts,201,567,234
2024-01-15T10:00:45Z,GET,/api/comments,200,4321,98
2024-01-15T10:00:50Z,DELETE,/api/posts/456,204,0,45
2024-01-15T10:00:55Z,GET,/admin,401,123,12
2024-01-15T10:01:00Z,GET,/api/search,200,7890,345
2024-01-15T10:01:05Z,POST,/api/upload,413,0,5
2024-01-15T10:01:10Z,GET,/api/stats,200,2345,234
2024-01-15T10:01:15Z,GET,/api/health,200,89,8
2024-01-15T10:01:20Z,GET,/api/users,500,567,1234
2024-01-15T10:01:25Z,GET,/api/retry,200,890,89
2024-01-15T10:01:30Z,PATCH,/api/users/789,200,345,123
2024-01-15T10:01:35Z,GET,/robots.txt,200,234,4
2024-01-15T10:01:40Z,GET,/api/feed,200,15678,456
2024-01-15T10:01:45Z,POST,/api/logout,200,123,34
2024-01-15T10:01:50Z,GET,/nonexistent,404,567,7
2024-01-15T10:01:55Z,GET,/api/metrics,200,3456,67
2024-01-15T10:02:00Z,GET,/api/version,200,89,12
EOF
success "Created simple_csv.csv (25 rows, headers)"

# 4. simple_tsv.tsv - TSV format, 20 rows of tabular data
cat > "$EXAMPLES_DIR/simple_tsv.tsv" <<'EOF'
user_id	username	action	timestamp	success
101	alice	login	2024-01-15T10:00:00Z	true
102	bob	login	2024-01-15T10:00:10Z	true
103	charlie	login	2024-01-15T10:00:20Z	false
101	alice	view_profile	2024-01-15T10:01:00Z	true
104	diana	login	2024-01-15T10:01:30Z	true
102	bob	edit_post	2024-01-15T10:02:00Z	true
101	alice	upload_file	2024-01-15T10:02:30Z	false
105	eve	login	2024-01-15T10:03:00Z	true
103	charlie	reset_password	2024-01-15T10:03:30Z	true
104	diana	delete_account	2024-01-15T10:04:00Z	true
102	bob	logout	2024-01-15T10:05:00Z	true
105	eve	view_dashboard	2024-01-15T10:05:30Z	true
101	alice	logout	2024-01-15T10:06:00Z	true
106	frank	login	2024-01-15T10:07:00Z	false
106	frank	login	2024-01-15T10:07:30Z	false
106	frank	login	2024-01-15T10:08:00Z	false
105	eve	change_settings	2024-01-15T10:09:00Z	true
103	charlie	login	2024-01-15T10:10:00Z	true
107	grace	login	2024-01-15T10:11:00Z	true
103	charlie	logout	2024-01-15T10:12:00Z	true
EOF
success "Created simple_tsv.tsv (20 rows, tab-separated)"

# 5. simple_logfmt.log - Logfmt format, 30 lines of structured data
cat > "$EXAMPLES_DIR/simple_logfmt.log" <<'EOF'
timestamp=2024-01-15T10:00:00Z level=info service=web msg="Server started" port=8080
timestamp=2024-01-15T10:00:05Z level=debug service=web msg="Loading middleware" count=5
timestamp=2024-01-15T10:00:10Z level=info service=db msg="Connection pool ready" size=20
timestamp=2024-01-15T10:01:00Z level=info service=web method=GET path=/api/users status=200 duration=45
timestamp=2024-01-15T10:01:05Z level=warn service=cache msg="Cache miss" key=user:123
timestamp=2024-01-15T10:01:10Z level=info service=web method=POST path=/api/posts status=201 duration=123
timestamp=2024-01-15T10:02:00Z level=error service=db msg="Query timeout" query="SELECT * FROM large_table" duration=5000
timestamp=2024-01-15T10:02:05Z level=info service=web method=GET path=/api/posts status=500 duration=5001
timestamp=2024-01-15T10:03:00Z level=debug service=auth msg="Token validated" user_id=42
timestamp=2024-01-15T10:03:30Z level=info service=web method=PUT path=/api/users/42 status=200 duration=67
timestamp=2024-01-15T10:04:00Z level=warn service=rate_limiter msg="Rate limit approaching" ip=10.0.0.5 current=95 limit=100
timestamp=2024-01-15T10:04:30Z level=error service=rate_limiter msg="Rate limit exceeded" ip=10.0.0.5 current=101 limit=100
timestamp=2024-01-15T10:05:00Z level=info service=web method=GET path=/api/data status=429 duration=2
timestamp=2024-01-15T10:06:00Z level=info service=scheduler msg="Job started" job_name=cleanup schedule="*/5 * * * *"
timestamp=2024-01-15T10:06:30Z level=debug service=scheduler msg="Processing batch" records=1000
timestamp=2024-01-15T10:07:00Z level=info service=scheduler msg="Job completed" job_name=cleanup duration=60000 records_processed=5000
timestamp=2024-01-15T10:08:00Z level=info service=metrics msg="Metrics snapshot" requests_total=15234 errors_total=42 avg_latency=78
timestamp=2024-01-15T10:09:00Z level=warn service=disk msg="Disk usage high" partition=/var usage_percent=87
timestamp=2024-01-15T10:10:00Z level=info service=web method=GET path=/health status=200 duration=3
timestamp=2024-01-15T10:11:00Z level=debug service=cache msg="Cache eviction" evicted_keys=150 reason=memory
timestamp=2024-01-15T10:12:00Z level=info service=web method=DELETE path=/api/posts/999 status=204 duration=34
timestamp=2024-01-15T10:13:00Z level=error service=external_api msg="API call failed" endpoint=https://api.partner.com status=503 retry=1
timestamp=2024-01-15T10:13:30Z level=info service=external_api msg="API call succeeded" endpoint=https://api.partner.com status=200 retry=2
timestamp=2024-01-15T10:14:00Z level=info service=web method=GET path=/api/export status=200 duration=2345 size_mb=15
timestamp=2024-01-15T10:15:00Z level=critical service=security msg="Suspicious activity detected" ip=203.0.113.5 pattern=sql_injection
timestamp=2024-01-15T10:15:01Z level=info service=security msg="IP blocked" ip=203.0.113.5 duration=3600
timestamp=2024-01-15T10:16:00Z level=info service=web method=GET path=/admin status=403 duration=5
timestamp=2024-01-15T10:17:00Z level=debug service=websocket msg="Client connected" client_id=ws_12345
timestamp=2024-01-15T10:18:00Z level=debug service=websocket msg="Client disconnected" client_id=ws_12345 duration=60
timestamp=2024-01-15T10:19:00Z level=info service=backup msg="Backup completed" size_gb=50 duration=3600000 destination=s3://backups/daily
EOF
success "Created simple_logfmt.log (30 lines, logfmt)"

# 6. simple_syslog.log - Use flog for RFC3164 syslog
if command -v flog &> /dev/null; then
    flog -f rfc3164 -n 25 -t stdout > "$EXAMPLES_DIR/simple_syslog.log"
    success "Created simple_syslog.log (25 lines, flog generated)"
else
    # Fallback if flog not available
    cat > "$EXAMPLES_DIR/simple_syslog.log" <<'EOF'
<34>Jan 15 10:00:00 webserver nginx: 192.168.1.10 - - [15/Jan/2024:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 612
<13>Jan 15 10:00:05 appserver myapp[1234]: User authentication successful for user: alice
<11>Jan 15 10:00:10 dbserver postgres[5432]: LOG: database system is ready to accept connections
<27>Jan 15 10:00:15 webserver nginx: 192.168.1.20 - - [15/Jan/2024:10:00:15 +0000] "POST /api/login HTTP/1.1" 200 234
<19>Jan 15 10:00:20 appserver myapp[1234]: Processing request for endpoint: /api/users
<14>Jan 15 10:00:25 loadbalancer haproxy: Server backend1/web1 is UP
<30>Jan 15 10:00:30 mailserver postfix[789]: connect from mail.example.com[192.0.2.1]
<134>Jan 15 10:00:35 firewall kernel: [UFW BLOCK] IN=eth0 OUT= SRC=198.51.100.5 DST=192.168.1.1
<86>Jan 15 10:00:40 vpnserver openvpn[2345]: user 'bob' authenticated
<27>Jan 15 10:00:45 webserver nginx: 192.168.1.30 - - [15/Jan/2024:10:00:45 +0000] "GET /api/data HTTP/1.1" 500 0
<19>Jan 15 10:00:50 appserver myapp[1234]: ERROR: Database query failed
<11>Jan 15 10:00:55 dbserver postgres[5432]: ERROR: connection timeout
<13>Jan 15 10:01:00 appserver myapp[1234]: Retrying database operation
<11>Jan 15 10:01:05 dbserver postgres[5432]: LOG: connection received
<19>Jan 15 10:01:10 appserver myapp[1234]: Database operation succeeded
<30>Jan 15 10:01:15 mailserver postfix[789]: message accepted
<14>Jan 15 10:01:20 loadbalancer haproxy: backend backend1 has no server available
<27>Jan 15 10:01:25 webserver nginx: 192.168.1.40 - - [15/Jan/2024:10:01:25 +0000] "GET /health HTTP/1.1" 503 0
<14>Jan 15 10:01:30 loadbalancer haproxy: Server backend1/web2 is UP
<27>Jan 15 10:01:35 webserver nginx: 192.168.1.50 - - [15/Jan/2024:10:01:35 +0000] "GET /health HTTP/1.1" 200 89
<13>Jan 15 10:01:40 cron CRON[9876]: (root) CMD (/usr/local/bin/backup.sh)
<13>Jan 15 10:01:45 backup backup.sh: Starting incremental backup
<13>Jan 15 10:02:00 backup backup.sh: Backup completed successfully
<86>Jan 15 10:02:05 vpnserver openvpn[2345]: user 'charlie' connection terminated
<134>Jan 15 10:02:10 firewall kernel: [UFW BLOCK] IN=eth0 OUT= SRC=203.0.113.42 DST=192.168.1.1
EOF
    success "Created simple_syslog.log (25 lines, manual)"
fi

# 7. simple_combined.log - Use flog for Apache/Nginx combined format
if command -v flog &> /dev/null; then
    flog -f apache_combined -n 40 -t stdout > "$EXAMPLES_DIR/simple_combined.log"
    success "Created simple_combined.log (40 lines, flog generated)"
else
    # Fallback
    cat > "$EXAMPLES_DIR/simple_combined.log" <<'EOF'
192.168.1.1 - - [15/Jan/2024:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 1234 "http://www.example.com/" "Mozilla/5.0 (Windows NT 10.0)"
127.0.0.1 - - [15/Jan/2024:10:00:05 +0000] "POST /api/users HTTP/1.1" 201 567 "-" "curl/7.68.0"
10.0.0.5 - alice [15/Jan/2024:10:00:10 +0000] "GET /dashboard HTTP/1.1" 200 8912 "http://app.example.com/login" "Mozilla/5.0 (X11; Linux x86_64)"
192.168.1.2 - - [15/Jan/2024:10:00:15 +0000] "GET /favicon.ico HTTP/1.1" 404 0 "-" "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"
192.168.1.3 - - [15/Jan/2024:10:00:20 +0000] "GET /api/posts HTTP/1.1" 200 5432 "http://app.example.com/" "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0)"
EOF
    success "Created simple_combined.log (5 lines, manual - install flog for more)"
fi

# 8. simple_cef.log - Common Event Format (CEF), 15 security events
cat > "$EXAMPLES_DIR/simple_cef.log" <<'EOF'
CEF:0|Acme|SIEM|1.0|100|User Login|5|src=192.168.1.10 suser=alice dhost=webserver.example.com outcome=success
CEF:0|Acme|SIEM|1.0|101|User Logout|3|src=192.168.1.10 suser=alice dhost=webserver.example.com
CEF:0|Acme|SIEM|1.0|102|Failed Login|7|src=203.0.113.5 suser=admin dhost=webserver.example.com outcome=failure reason=invalid_password attempts=3
CEF:0|Acme|SIEM|1.0|103|File Access|5|src=192.168.1.20 suser=bob fname=/etc/passwd outcome=success
CEF:0|Acme|SIEM|1.0|104|Permission Denied|6|src=192.168.1.30 suser=charlie fname=/root/secrets.txt outcome=failure
CEF:0|Acme|SIEM|1.0|105|SQL Injection Attempt|9|src=198.51.100.10 request=GET /api/users?id=1' OR '1'='1 outcome=blocked
CEF:0|Acme|SIEM|1.0|106|Malware Detected|10|src=192.168.1.40 suser=diana fname=/tmp/malware.exe hash=d41d8cd98f00b204e9800998ecf8427e outcome=quarantined
CEF:0|Acme|SIEM|1.0|107|Port Scan|8|src=203.0.113.42 dst=192.168.1.1 dpt=1-1024 outcome=blocked
CEF:0|Acme|SIEM|1.0|108|Privilege Escalation|9|src=192.168.1.50 suser=eve duser=root outcome=failure
CEF:0|Acme|SIEM|1.0|109|Data Exfiltration|10|src=192.168.1.60 suser=frank bytes=104857600 dst=198.51.100.50 outcome=blocked
CEF:0|Acme|SIEM|1.0|110|Firewall Rule Change|6|src=192.168.1.70 suser=admin msg=Added rule: ALLOW 203.0.113.0/24
CEF:0|Acme|SIEM|1.0|111|Account Lockout|7|suser=admin dhost=webserver.example.com reason=too_many_failed_attempts lockout_duration=1800
CEF:0|Acme|SIEM|1.0|112|Suspicious Download|8|src=192.168.1.80 suser=grace fname=hacking_tools.zip outcome=flagged
CEF:0|Acme|SIEM|1.0|113|VPN Connection|5|src=203.0.113.100 suser=henry dst=vpn.example.com outcome=success
CEF:0|Acme|SIEM|1.0|114|Certificate Expiry Warning|6|dhost=api.example.com msg=Certificate expires in 7 days
EOF
success "Created simple_cef.log (15 events, CEF format)"

################################################################################
# ADVANCED FORMAT FEATURES (6 files)
################################################################################

log "Generating advanced format features files..."

# 9. cols_fixed.log - Fixed-width columns for cols: parser testing
cat > "$EXAMPLES_DIR/cols_fixed.log" <<'EOF'
2024-01-15 10:00:00 INFO     server  Application started successfully
2024-01-15 10:00:05 DEBUG    config  Loading configuration from file
2024-01-15 10:00:10 WARN     memory  Memory usage at 85% threshold
2024-01-15 10:00:15 ERROR    network Connection timeout after 5000ms
2024-01-15 10:00:20 INFO     api     Received GET request for /users
2024-01-15 10:00:25 DEBUG    cache   Cache hit for key user:123
2024-01-15 10:00:30 INFO     api     Response sent with status 200
2024-01-15 10:00:35 CRITICAL disk    Disk space below 1GB on /var
2024-01-15 10:00:40 ERROR    auth    Invalid credentials for admin
2024-01-15 10:00:45 WARN     rate    Rate limit approaching for IP
EOF
success "Created cols_fixed.log (10 lines, fixed-width columns)"

# 10. cols_mixed.log - Mixed whitespace-separated columns with special chars
cat > "$EXAMPLES_DIR/cols_mixed.log" <<'EOF'
alice@example.com    2024-01-15T10:00:00Z    login_success    192.168.1.10
bob@test.org         2024-01-15T10:01:00Z    view_page       10.0.0.5
charlie+spam@mail.com 2024-01-15T10:02:00Z   failed_login    203.0.113.42
diana.smith@company.net 2024-01-15T10:03:00Z logout          192.168.1.20
eve_admin@internal     2024-01-15T10:04:00Z  privilege_escalation 172.16.0.1
frank.jones@sub.domain.com 2024-01-15T10:05:00Z api_call    10.1.2.3
grace-user@test.co.uk  2024-01-15T10:06:00Z  file_upload     192.168.2.50
henry123@numbers.com   2024-01-15T10:07:00Z  password_reset  127.0.0.1
iris.o'brien@irish.ie  2024-01-15T10:08:00Z  login_success   192.168.1.30
jack_test@localhost    2024-01-15T10:09:00Z  logout          ::1
EOF
success "Created cols_mixed.log (10 lines, mixed columns with special characters)"

# 11. csv_typed.csv - CSV with type annotations (status:int bytes:int)
cat > "$EXAMPLES_DIR/csv_typed.csv" <<'EOF'
timestamp,method,path,status:int,bytes:int,duration:int
2024-01-15T10:00:00Z,GET,/,200,1234,45
2024-01-15T10:00:05Z,POST,/api/login,200,456,78
2024-01-15T10:00:10Z,GET,/api/users,200,9876,123
2024-01-15T10:00:15Z,PUT,/api/users/123,200,567,89
2024-01-15T10:00:20Z,GET,/favicon.ico,404,0,5
2024-01-15T10:00:25Z,DELETE,/api/posts/999,204,0,34
2024-01-15T10:00:30Z,GET,/admin,401,234,12
2024-01-15T10:00:35Z,POST,/api/upload,413,0,8
2024-01-15T10:00:40Z,GET,/api/search,200,5432,345
2024-01-15T10:00:45Z,GET,/api/export,200,123456,2345
2024-01-15T10:00:50Z,PATCH,/api/settings,200,789,67
2024-01-15T10:00:55Z,GET,/api/stats,500,0,1234
2024-01-15T10:01:00Z,GET,/api/health,200,89,8
2024-01-15T10:01:05Z,POST,/api/webhook,202,123,56
2024-01-15T10:01:10Z,GET,/robots.txt,200,345,4
EOF
success "Created csv_typed.csv (15 rows, type annotations)"

# 12. prefix_docker.log - Docker compose logs with container prefixes
cat > "$EXAMPLES_DIR/prefix_docker.log" <<'EOF'
web_1        | 2024-01-15 10:00:00 [INFO] Starting web server on port 8080
web_1        | 2024-01-15 10:00:05 [INFO] Listening for connections
db_1         | 2024-01-15 10:00:10 [INFO] PostgreSQL starting up
db_1         | 2024-01-15 10:00:15 [INFO] Database system is ready
redis_1      | 2024-01-15 10:00:20 [INFO] Redis server starting
redis_1      | 2024-01-15 10:00:25 [INFO] Ready to accept connections
web_1        | 2024-01-15 10:00:30 [INFO] Connected to database
web_1        | 2024-01-15 10:00:35 [DEBUG] Cache connection established
nginx_1      | 2024-01-15 10:00:40 [INFO] Nginx starting
nginx_1      | 2024-01-15 10:00:45 [INFO] Ready to serve traffic
web_1        | 2024-01-15 10:01:00 [INFO] Received request GET /api/health
web_1        | 2024-01-15 10:01:05 [INFO] Health check passed
db_1         | 2024-01-15 10:01:10 [WARN] Connection pool 80% utilized
redis_1      | 2024-01-15 10:01:15 [WARN] Memory usage at 75%
worker_1     | 2024-01-15 10:01:20 [INFO] Background worker starting
worker_1     | 2024-01-15 10:01:25 [INFO] Processing job queue
web_1        | 2024-01-15 10:01:30 [ERROR] Request timeout after 5s
nginx_1      | 2024-01-15 10:01:35 [ERROR] Upstream server timeout
worker_1     | 2024-01-15 10:01:40 [INFO] Job completed: send_email
db_1         | 2024-01-15 10:01:45 [INFO] Checkpoint completed
EOF
success "Created prefix_docker.log (20 lines, container prefixes)"

# 13. prefix_custom.log - Custom multi-char separator prefix extraction
cat > "$EXAMPLES_DIR/prefix_custom.log" <<'EOF'
[node-1] >>> 2024-01-15T10:00:00Z Cluster node started
[node-1] >>> 2024-01-15T10:00:05Z Joining cluster at 192.168.1.100
[node-2] >>> 2024-01-15T10:00:10Z Cluster node started
[node-2] >>> 2024-01-15T10:00:15Z Joining cluster at 192.168.1.100
[node-1] >>> 2024-01-15T10:00:20Z Connected to 1 peer(s)
[node-2] >>> 2024-01-15T10:00:25Z Connected to 1 peer(s)
[node-3] >>> 2024-01-15T10:00:30Z Cluster node started
[node-3] >>> 2024-01-15T10:00:35Z Joining cluster at 192.168.1.100
[node-1] >>> 2024-01-15T10:00:40Z Connected to 2 peer(s)
[node-2] >>> 2024-01-15T10:00:45Z Connected to 2 peer(s)
[node-3] >>> 2024-01-15T10:00:50Z Connected to 2 peer(s)
[node-1] >>> 2024-01-15T10:01:00Z Leader election starting
[node-1] >>> 2024-01-15T10:01:05Z Elected as cluster leader
[node-2] >>> 2024-01-15T10:01:10Z Following leader: node-1
[node-3] >>> 2024-01-15T10:01:15Z Following leader: node-1
[node-1] >>> 2024-01-15T10:01:20Z Cluster ready with 3 nodes
[node-2] >>> 2024-01-15T10:01:25Z Synchronization complete
[node-3] >>> 2024-01-15T10:01:30Z Synchronization complete
[node-1] >>> 2024-01-15T10:01:35Z Processing distributed task #1
[node-2] >>> 2024-01-15T10:01:40Z Processing distributed task #2
EOF
success "Created prefix_custom.log (20 lines, custom separator)"

# 14. kv_pairs.log - Mixed key-value formats for parse_kv testing
cat > "$EXAMPLES_DIR/kv_pairs.log" <<'EOF'
user=alice action=login timestamp=2024-01-15T10:00:00Z success=true ip=192.168.1.10
user=bob action=view_page timestamp=2024-01-15T10:01:00Z page=/dashboard duration=1.5
user=charlie action=api_call timestamp=2024-01-15T10:02:00Z endpoint=/api/users method=GET status=200
user=diana action=file_upload timestamp=2024-01-15T10:03:00Z filename=document.pdf size=1048576 success=true
user=eve action=failed_login timestamp=2024-01-15T10:04:00Z attempts=3 locked=true ip=203.0.113.5
user=frank action=password_reset timestamp=2024-01-15T10:05:00Z email=frank@example.com token_sent=true
user=grace action=logout timestamp=2024-01-15T10:06:00Z session_duration=3600 reason=manual
user=henry action=api_call timestamp=2024-01-15T10:07:00Z endpoint=/api/export method=POST bytes=5242880
user=iris action=privilege_escalation timestamp=2024-01-15T10:08:00Z from=user to=admin success=false
user=jack action=delete_account timestamp=2024-01-15T10:09:00Z confirmed=true data_removed=true
EOF
success "Created kv_pairs.log (10 lines, key-value pairs)"

################################################################################
# MULTILINE SCENARIOS (5 files)
################################################################################

log "Generating multiline scenario files..."

# 15. multiline_stacktrace.log - Java/Python stacktraces with timestamps
cat > "$EXAMPLES_DIR/multiline_stacktrace.log" <<'EOF'
2024-01-15 10:00:00 INFO Starting application
2024-01-15 10:00:05 DEBUG Initializing database connection
2024-01-15 10:01:00 ERROR Failed to process request
Traceback (most recent call last):
  File "/app/server.py", line 42, in handle_request
    result = process_data(request.body)
  File "/app/processor.py", line 15, in process_data
    return json.loads(data)
ValueError: Invalid JSON format at line 3
2024-01-15 10:01:30 WARN Retrying with default configuration
2024-01-15 10:02:00 ERROR Database connection failed
java.sql.SQLException: Connection timeout
	at com.example.db.ConnectionPool.getConnection(ConnectionPool.java:123)
	at com.example.api.UserController.getUser(UserController.java:45)
	at com.example.api.RequestHandler.handle(RequestHandler.java:89)
Caused by: java.net.SocketTimeoutException: Read timed out
	at java.net.SocketInputStream.socketRead0(Native Method)
	at java.net.SocketInputStream.read(SocketInputStream.java:150)
2024-01-15 10:02:30 INFO Switching to backup database
2024-01-15 10:03:00 ERROR Unhandled exception in worker thread
RuntimeError: Maximum retry attempts exceeded
  File "/app/worker.py", line 67, in run
    self.process_job(job)
  File "/app/worker.py", line 98, in process_job
    raise RuntimeError("Maximum retry attempts exceeded")
2024-01-15 10:03:30 CRITICAL System shutting down due to errors
EOF
success "Created multiline_stacktrace.log (stacktraces with timestamps)"

# 16. multiline_json_arrays.log - Pretty-printed JSON events
cat > "$EXAMPLES_DIR/multiline_json_arrays.log" <<'EOF'
{
  "timestamp": "2024-01-15T10:00:00Z",
  "event": "user_action",
  "user": {
    "id": 123,
    "name": "alice"
  },
  "actions": [
    "login",
    "view_dashboard",
    "edit_profile"
  ]
}
{
  "timestamp": "2024-01-15T10:01:00Z",
  "event": "api_call",
  "endpoint": "/api/users",
  "response": {
    "status": 200,
    "data": {
      "count": 42,
      "users": ["alice", "bob", "charlie"]
    }
  }
}
{
  "timestamp": "2024-01-15T10:02:00Z",
  "event": "error",
  "error": {
    "type": "DatabaseError",
    "message": "Connection failed",
    "stack": [
      "db.connect()",
      "app.init()",
      "main()"
    ]
  },
  "retries": 3
}
EOF
success "Created multiline_json_arrays.log (pretty-printed JSON)"

# 17. multiline_continuation.log - Lines with backslash continuation
cat > "$EXAMPLES_DIR/multiline_continuation.log" <<'EOF'
command: docker run -d \
  --name myapp \
  --memory 2g \
  --cpus 2 \
  -p 8080:8080 \
  -v /data:/app/data \
  myapp:latest
result: Container started successfully
command: kubectl apply -f - <<EOF \
apiVersion: v1 \
kind: Service \
metadata: \
  name: myservice \
spec: \
  selector: \
    app: myapp \
  ports: \
  - port: 80
result: Service created
query: SELECT users.name, orders.total \
FROM users \
INNER JOIN orders ON users.id = orders.user_id \
WHERE orders.status = 'completed' \
  AND orders.total > 100 \
ORDER BY orders.total DESC \
LIMIT 10
rows: 7
config: export DATABASE_URL="postgresql://user:pass@localhost/db" \
       export REDIS_URL="redis://localhost:6379" \
       export SECRET_KEY="very-long-secret-key-that-needs-continuation" \
       export API_ENDPOINT="https://api.example.com/v1"
loaded: true
EOF
success "Created multiline_continuation.log (backslash continuation)"

# 18. multiline_boundary.log - BEGIN/END block delimiters
cat > "$EXAMPLES_DIR/multiline_boundary.log" <<'EOF'
BEGIN
type: database_backup
timestamp: 2024-01-15T10:00:00Z
status: started
target: postgresql://production
END
BEGIN
type: database_backup
timestamp: 2024-01-15T10:05:00Z
status: completed
size_mb: 1024
duration_seconds: 300
files: backup_20240115_100000.sql.gz
checksum: a1b2c3d4e5f6
END
BEGIN
type: deployment
timestamp: 2024-01-15T10:10:00Z
status: started
version: v1.2.3
environment: production
services:
  - web
  - worker
  - scheduler
END
BEGIN
type: deployment
timestamp: 2024-01-15T10:15:00Z
status: failed
version: v1.2.3
environment: production
error: Health check timeout
rollback: true
END
BEGIN
type: alert
timestamp: 2024-01-15T10:15:30Z
severity: high
message: Deployment failed, rolled back to v1.2.2
recipients:
  - oncall@example.com
  - devops@example.com
END
EOF
success "Created multiline_boundary.log (BEGIN/END blocks)"

# 19. multiline_indent.log - Indented log entries (YAML-style)
cat > "$EXAMPLES_DIR/multiline_indent.log" <<'EOF'
event: application_start
timestamp: 2024-01-15T10:00:00Z
config:
  port: 8080
  workers: 4
  timeout: 30
  database:
    host: localhost
    port: 5432
    pool_size: 20
status: success
event: request_received
timestamp: 2024-01-15T10:01:00Z
request:
  method: POST
  path: /api/users
  headers:
    content-type: application/json
    user-agent: curl/7.68.0
  body:
    name: alice
    email: alice@example.com
response:
  status: 201
  duration_ms: 45
event: error_occurred
timestamp: 2024-01-15T10:02:00Z
error:
  type: ValidationError
  message: Invalid email format
  field: email
  value: not-an-email
  context:
    user_id: 123
    ip: 192.168.1.10
action: rejected
event: scheduled_job
timestamp: 2024-01-15T10:03:00Z
job:
  name: cleanup_old_logs
  schedule: "0 2 * * *"
  params:
    retention_days: 30
    directory: /var/log/app
result:
  deleted_files: 157
  freed_space_mb: 2048
  duration_seconds: 12
status: completed
EOF
success "Created multiline_indent.log (indented YAML-style)"

################################################################################
# COMPLEX REAL-WORLD DATA (5 files)
################################################################################

log "Generating complex real-world data files..."

# 20. web_access_large.log.gz - 1000+ combined format entries for parallel testing (gzipped)
# Note: flog -n stamps every line with the single invocation time, so the raw
# output shares one timestamp. We respace timestamps across a realistic ~2h
# window so --discover shows a time spread and --since/--until are testable.
if command -v flog &> /dev/null && command -v python3 &> /dev/null; then
    flog -f apache_combined -n 1200 -t stdout \
        | python3 "$SCRIPT_DIR/respace_apache_timestamps.py" \
        | gzip > "$EXAMPLES_DIR/web_access_large.log.gz"
    success "Created web_access_large.log.gz (1200 lines, flog generated, timestamps respaced, gzipped)"
else
    log "flog not available, generating smaller synthetic file..."
    {
        for i in {1..1200}; do
            ip="192.168.$((i % 256)).$((i % 256))"
            hour=$((10 + i / 300))
            minute=$((i % 60))
            status=$((i % 10 < 8 ? 200 : (i % 10 == 8 ? 404 : 500)))
            bytes=$((100 + i * 10))
            echo "$ip - - [15/Jan/2024:$hour:$minute:00 +0000] \"GET /page$i HTTP/1.1\" $status $bytes \"-\" \"Mozilla/5.0\""
        done
    } | gzip > "$EXAMPLES_DIR/web_access_large.log.gz"
    success "Created web_access_large.log.gz (1200 lines, synthetic, gzipped)"
fi

# 21. json_nested_deep.jsonl - Deeply nested JSON for get_path/has_path
cat > "$EXAMPLES_DIR/json_nested_deep.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","request":{"user":{"profile":{"name":"alice","settings":{"theme":"dark","notifications":{"email":true,"push":false}}},"id":123},"metadata":{"ip":"192.168.1.10","session":"sess_abc123"}}}
{"timestamp":"2024-01-15T10:01:00Z","request":{"user":{"profile":{"name":"bob","settings":{"theme":"light"}},"id":456},"metadata":{"ip":"10.0.0.5"}}}
{"timestamp":"2024-01-15T10:02:00Z","order":{"customer":{"name":"charlie","address":{"street":"123 Main St","city":"Portland","state":"OR","zip":"97201","coordinates":{"lat":45.5231,"lon":-122.6765}}},"items":[{"product":{"name":"Widget","sku":"WDG-001","price":29.99},"quantity":2},{"product":{"name":"Gadget","sku":"GDG-002","price":49.99},"quantity":1}],"total":109.97}}
{"timestamp":"2024-01-15T10:03:00Z","error":{"type":"ValidationError","details":{"field":"email","message":"Invalid format","context":{"user":{"input":"not-an-email","expected":"email@example.com"},"validation":{"rules":{"pattern":"^[^@]+@[^@]+$","required":true}}}}}}
{"timestamp":"2024-01-15T10:04:00Z","api":{"version":"v2","endpoint":"/graphql","query":{"operation":"getUser","variables":{"userId":789},"fields":["id","name",{"profile":["bio","avatar",{"social":["twitter","github"]}]}]},"response":{"data":{"user":{"id":789,"name":"diana","profile":{"bio":"Software engineer","avatar":"https://example.com/avatar.jpg","social":{"twitter":"@diana","github":"diana-dev"}}}}}}}
{"timestamp":"2024-01-15T10:05:00Z","trace":{"service":"api","span":{"id":"span_001","parent":"span_000","operation":"database.query","tags":{"db.type":"postgresql","db.statement":"SELECT * FROM users WHERE id = $1"},"logs":[{"timestamp":"2024-01-15T10:05:00.100Z","level":"debug","message":"Query started"},{"timestamp":"2024-01-15T10:05:00.145Z","level":"info","message":"Query completed","fields":{"rows":1,"duration_ms":45}}]}}}
{"timestamp":"2024-01-15T10:06:00Z","deployment":{"metadata":{"version":"v1.2.3","commit":"abc123def456","branch":"main","author":{"name":"Eve","email":"eve@example.com"}},"environment":{"name":"production","region":"us-west-2","infrastructure":{"kubernetes":{"cluster":"prod-cluster","namespace":"default","pods":{"web":{"replicas":5,"image":"myapp:v1.2.3"},"worker":{"replicas":3,"image":"myapp-worker:v1.2.3"}}}}},"status":"success"}}
{"timestamp":"2024-01-15T10:07:00Z","metrics":{"application":{"requests":{"total":15234,"success":15120,"errors":{"4xx":89,"5xx":25,"breakdown":{"400":23,"401":45,"404":21,"500":15,"502":7,"503":3}}},"latency":{"p50":45,"p95":234,"p99":567,"max":1234},"throughput":{"requests_per_second":127,"bytes_per_second":1048576}}}}
EOF
success "Created json_nested_deep.jsonl (8 events, deeply nested)"

# 22. json_arrays.jsonl - Events with arrays for emit_each/sorted/unique
cat > "$EXAMPLES_DIR/json_arrays.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","batch_id":"batch_001","users":[{"id":1,"name":"alice","score":95},{"id":2,"name":"bob","score":87},{"id":3,"name":"charlie","score":92}]}
{"timestamp":"2024-01-15T10:01:00Z","batch_id":"batch_002","users":[{"id":4,"name":"diana","score":88},{"id":5,"name":"eve","score":94}]}
{"timestamp":"2024-01-15T10:02:00Z","event":"pageview","tags":["homepage","mobile","campaign_summer"],"scores":[85,92,78,95,88,91]}
{"timestamp":"2024-01-15T10:03:00Z","event":"search","tags":["search","web","logged_in"],"queries":["laptop","wireless mouse","laptop","keyboard","laptop"]}
{"timestamp":"2024-01-15T10:04:00Z","batches":[{"name":"batch_a","items":[{"id":1,"status":"active","priority":"high"},{"id":2,"status":"inactive","priority":"low"}]},{"name":"batch_b","items":[{"id":3,"status":"active","priority":"high"},{"id":4,"status":"active","priority":"medium"}]}]}
{"timestamp":"2024-01-15T10:05:00Z","data":{"values":[100,200,150,300,250,180],"labels":["Jan","Feb","Mar","Apr","May","Jun"]}}
{"timestamp":"2024-01-15T10:06:00Z","logs":[{"level":"info","msg":"Started"},{"level":"debug","msg":"Processing"},{"level":"warn","msg":"Slow query"},{"level":"info","msg":"Completed"}]}
{"timestamp":"2024-01-15T10:07:00Z","emails":["alice@example.com","bob@test.org","charlie@example.com","alice@example.com","diana@example.com"],"domains":["example.com","example.com","test.org","example.com"]}
{"timestamp":"2024-01-15T10:08:00Z","matrix":[[1,2,3],[4,5,6],[7,8,9]],"flattened":[1,2,3,4,5,6,7,8,9]}
{"timestamp":"2024-01-15T10:09:00Z","purchases":[{"item":"laptop","price":999,"qty":1},{"item":"mouse","price":29,"qty":2},{"item":"keyboard","price":89,"qty":1}],"total":1146}
EOF
success "Created json_arrays.jsonl (10 events, various arrays)"

# 23. security_audit.jsonl - Mixed IPs, JWTs, hashes for security functions
cat > "$EXAMPLES_DIR/security_audit.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","event":"login","user":"alice","ip":"192.168.1.10","success":true,"token":"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkFsaWNlIiwicm9sZSI6InVzZXIiLCJpYXQiOjE1MTYyMzkwMjJ9.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"}
{"timestamp":"2024-01-15T10:00:10Z","event":"login","user":"admin","ip":"203.0.113.42","success":false,"reason":"invalid_password","attempts":3}
{"timestamp":"2024-01-15T10:00:20Z","event":"api_call","ip":"10.0.0.5","endpoint":"/api/users","method":"GET","token":"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiI5ODc2NTQzMjEiLCJuYW1lIjoiQm9iIiwicm9sZSI6ImFkbWluIiwiaWF0IjoxNTE2MjM5MDIyfQ.dQw4w9WgXcQ"}
{"timestamp":"2024-01-15T10:00:30Z","event":"file_upload","user":"charlie","ip":"172.16.0.10","file":"document.pdf","hash":"5d41402abc4b2a76b9719d911017c592","size":1048576}
{"timestamp":"2024-01-15T10:00:40Z","event":"suspicious_activity","ip":"198.51.100.50","type":"port_scan","ports":[22,23,80,443,3306,5432,8080],"blocked":true}
{"timestamp":"2024-01-15T10:00:50Z","event":"login","user":"diana","ip":"2001:db8::1","success":true,"ipv6":true}
{"timestamp":"2024-01-15T10:01:00Z","event":"password_change","user":"eve","ip":"192.168.1.20","old_hash":"5f4dcc3b5aa765d61d8327deb882cf99","new_hash":"098f6bcd4621d373cade4e832627b4f6"}
{"timestamp":"2024-01-15T10:01:10Z","event":"api_call","ip":"10.255.255.1","endpoint":"/api/admin/users","method":"DELETE","user":"frank","blocked":true,"reason":"insufficient_privileges"}
{"timestamp":"2024-01-15T10:01:20Z","event":"vpn_connect","user":"grace","client_ip":"203.0.113.100","vpn_ip":"10.8.0.5","protocol":"openvpn"}
{"timestamp":"2024-01-15T10:01:30Z","event":"data_export","user":"henry","ip":"192.168.2.30","records":10000,"hash":"e4d909c290d0fb1ca068ffaddf22cbd0","approved":true}
{"timestamp":"2024-01-15T10:01:40Z","event":"firewall_block","src_ip":"185.220.101.42","dst_ip":"192.168.1.1","dst_port":22,"reason":"blacklist","country":"unknown"}
{"timestamp":"2024-01-15T10:01:50Z","event":"certificate_renewal","domain":"api.example.com","issuer":"Let's Encrypt","fingerprint":"sha256:a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2"}
{"timestamp":"2024-01-15T10:02:00Z","event":"malware_scan","file":"/uploads/suspicious.exe","hash":"44d88612fea8a8f36de82e1278abb02f","threat":"trojan.generic","quarantined":true}
{"timestamp":"2024-01-15T10:02:10Z","event":"ddos_attempt","src_ips":["203.0.113.1","203.0.113.2","203.0.113.3"],"requests_per_second":5000,"mitigated":true}
{"timestamp":"2024-01-15T10:02:20Z","event":"ssh_login","user":"root","ip":"192.168.1.100","key_fingerprint":"SHA256:abcd1234efgh5678ijkl9012mnop3456","success":true}
EOF
success "Created security_audit.jsonl (15 events, security data)"

# 24. timezones_mixed.log - Various timestamp formats and timezones
cat > "$EXAMPLES_DIR/timezones_mixed.log" <<'EOF'
2024-01-15T10:00:00Z Event from UTC timezone
2024-01-15T10:00:00+00:00 Event with explicit UTC offset
2024-01-15T03:00:00-07:00 Event from US Mountain Time
2024-01-15T11:00:00+01:00 Event from Central European Time
2024-01-15T19:00:00+09:00 Event from Japan Standard Time
2024-01-15 10:00:00 Event with naive timestamp (no timezone)
Jan 15 10:00:00 Syslog-style timestamp without year
15/Jan/2024:10:00:00 +0000 Apache-style timestamp
2024-01-15 10:00:00.123 Event with milliseconds
2024-01-15 10:00:00.123456 Event with microseconds
2024-01-15T10:00:00.123456789Z Event with nanoseconds
Mon Jan 15 10:00:00 2024 Ctime-style timestamp
2024-01-15T10:00:00-08:00 Event from US Pacific Time
2024-01-15T05:30:00+05:30 Event from India Standard Time
2024-01-15T10:00:00.500Z Half-second precision event
EOF
success "Created timezones_mixed.log (15 lines, various formats)"

################################################################################
# ERROR HANDLING & RESILIENCE (7 files)
################################################################################

log "Generating error handling and resilience files..."

# 25. errors_json_mixed.jsonl - Valid JSON mixed with malformed
cat > "$EXAMPLES_DIR/errors_json_mixed.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","level":"INFO","message":"Valid event 1"}
{"timestamp":"2024-01-15T10:00:05Z","level":"INFO","message":"Valid event 2"}
{"timestamp":"2024-01-15T10:00:10Z","level":"ERROR","message":"Missing closing brace"
{"timestamp":"2024-01-15T10:00:15Z","level":"INFO","message":"Valid event 3"}
{"timestamp":"2024-01-15T10:00:20Z","level":"WARN","message":"Extra comma",}
{"timestamp":"2024-01-15T10:00:25Z","level":"INFO","message":"Valid event 4"}
{timestamp:"2024-01-15T10:00:30Z","level":"ERROR","message":"Unquoted key"}
{"timestamp":"2024-01-15T10:00:35Z","level":"INFO","message":"Valid event 5"}
{"timestamp":"2024-01-15T10:00:40Z","level":"WARN","message":"Trailing comma in array","tags":["tag1","tag2",]}
{"timestamp":"2024-01-15T10:00:45Z","level":"INFO","message":"Valid event 6"}
Not JSON at all - just plain text
{"timestamp":"2024-01-15T10:00:55Z","level":"INFO","message":"Valid event 7"}
{"timestamp":"2024-01-15T10:01:00Z","level":"ERROR","message":"Single quotes instead of double",'invalid':true}
{"timestamp":"2024-01-15T10:01:05Z","level":"INFO","message":"Valid event 8"}
{"timestamp":"2024-01-15T10:01:10Z"level":"WARN","message":"Missing comma between fields"}
{"timestamp":"2024-01-15T10:01:15Z","level":"INFO","message":"Valid event 9"}
{"timestamp":"2024-01-15T10:01:20Z","level":"INFO","message":"Valid event 10"}
EOF
success "Created errors_json_mixed.jsonl (17 lines, 7 valid, 10 malformed)"

# 26. errors_json_types.jsonl - Type mismatches for conversion function testing
cat > "$EXAMPLES_DIR/errors_json_types.jsonl" <<'EOF'
{"id":"123","count":"not-a-number","ratio":"invalid","enabled":"yes"}
{"id":456,"count":789,"ratio":0.5,"enabled":true}
{"id":"abc","count":"xyz","ratio":"NaN","enabled":"maybe"}
{"id":789,"count":"100","ratio":"0.75","enabled":"true"}
{"id":null,"count":null,"ratio":null,"enabled":null}
{"id":"","count":"","ratio":"","enabled":""}
{"id":"999","count":"50","ratio":"1.5","enabled":"false"}
{"id":"mixed","count":42,"ratio":"not-float","enabled":1}
{"id":111,"count":"222","ratio":0.333,"enabled":"on"}
{"id":"last","count":"infinity","ratio":"3.14159","enabled":"off"}
EOF
success "Created errors_json_types.jsonl (10 events, type conversion challenges)"

# 27. errors_empty_lines.log - Mix of empty lines, whitespace-only, valid entries
cat > "$EXAMPLES_DIR/errors_empty_lines.log" <<'EOF'
2024-01-15 10:00:00 INFO First valid line

2024-01-15 10:00:05 DEBUG Second valid line

2024-01-15 10:00:10 WARN Third valid line with whitespace around


2024-01-15 10:00:15 ERROR Fourth valid line after tabs


2024-01-15 10:00:20 INFO Fifth valid line



2024-01-15 10:00:25 INFO Sixth valid line after many blanks

2024-01-15 10:00:30 CRITICAL Last valid line
EOF
success "Created errors_empty_lines.log (mixed empty lines and whitespace)"

# 28. errors_csv_ragged.csv - CSV with inconsistent column counts
cat > "$EXAMPLES_DIR/errors_csv_ragged.csv" <<'EOF'
timestamp,level,message,extra
2024-01-15T10:00:00Z,INFO,Valid 4-column row,data
2024-01-15T10:00:05Z,WARN,Missing extra column
2024-01-15T10:00:10Z,ERROR,Extra,column,here,too,many
2024-01-15T10:00:15Z,INFO,Valid row,more_data
2024-01-15T10:00:20Z,DEBUG
2024-01-15T10:00:25Z,INFO,Another valid,row
2024-01-15T10:00:30Z,WARN,Quoted "comma, inside",value
2024-01-15T10:00:35Z,ERROR,Missing,
2024-01-15T10:00:40Z,INFO,Valid,data
2024-01-15T10:00:45Z,CRITICAL,Final,row,good
EOF
success "Created errors_csv_ragged.csv (inconsistent columns)"

# 29. errors_unicode.log - Invalid UTF-8, mixed encodings, special chars
cat > "$EXAMPLES_DIR/errors_unicode.log" <<'EOF'
2024-01-15 10:00:00 INFO ASCII text is fine
2024-01-15 10:00:05 INFO Unicode is great: ñoño 日本語 emoji 🚀
2024-01-15 10:00:10 WARN Special chars: <>&"'
2024-01-15 10:00:15 ERROR Control chars:
2024-01-15 10:00:20 INFO Zero-width: a​b​c (has zero-width spaces)
2024-01-15 10:00:25 WARN RTL text: مرحبا hello שלום
2024-01-15 10:00:30 INFO Math symbols: ∑∫√π≠≈∞
2024-01-15 10:00:35 DEBUG Box drawing: ┌─┐│└┘
2024-01-15 10:00:40 INFO Combining: é (e + ́) vs é (single char)
2024-01-15 10:00:45 WARN Surrogate pairs: 𝕳𝖊𝖑𝖑𝖔 𝔚𝔬𝔯𝔩𝔡
EOF
success "Created errors_unicode.log (Unicode and special characters)"

# 30. errors_filter_runtime.jsonl - Data triggering Rhai filter errors
cat > "$EXAMPLES_DIR/errors_filter_runtime.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","numerator":100,"denominator":10,"result":10}
{"timestamp":"2024-01-15T10:00:05Z","numerator":50,"denominator":0,"result":null}
{"timestamp":"2024-01-15T10:00:10Z","numerator":75,"denominator":5,"result":15}
{"timestamp":"2024-01-15T10:00:15Z","value":"not-a-number","doubled":null}
{"timestamp":"2024-01-15T10:00:20Z","value":42,"doubled":84}
{"timestamp":"2024-01-15T10:00:25Z","user":null,"name":null}
{"timestamp":"2024-01-15T10:00:30Z","user":{"name":"alice"},"name":"alice"}
{"timestamp":"2024-01-15T10:00:35Z","items":[],"first":null}
{"timestamp":"2024-01-15T10:00:40Z","items":["a","b","c"],"first":"a"}
{"timestamp":"2024-01-15T10:00:45Z","missing_field":true}
EOF
success "Created errors_filter_runtime.jsonl (runtime error triggers)"

# 31. errors_exec_transform.jsonl - Data causing transformation failures
cat > "$EXAMPLES_DIR/errors_exec_transform.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","status":"200","bytes":"1234"}
{"timestamp":"2024-01-15T10:00:05Z","status":"not-a-number","bytes":"invalid"}
{"timestamp":"2024-01-15T10:00:10Z","status":"404","bytes":"567"}
{"timestamp":"2024-01-15T10:00:15Z","tags":["a","b","c"]}
{"timestamp":"2024-01-15T10:00:20Z","tags":"not-an-array"}
{"timestamp":"2024-01-15T10:00:25Z","tags":["x","y","z"]}
{"timestamp":"2024-01-15T10:00:30Z","nested":{"value":42}}
{"timestamp":"2024-01-15T10:00:35Z","nested":"not-an-object"}
{"timestamp":"2024-01-15T10:00:40Z","nested":{"value":99}}
{"timestamp":"2024-01-15T10:00:45Z","url":"https://example.com/path"}
{"timestamp":"2024-01-15T10:00:50Z","url":"not a valid url"}
{"timestamp":"2024-01-15T10:00:55Z","url":"https://test.org/page"}
EOF
success "Created errors_exec_transform.jsonl (transformation error triggers)"

################################################################################
# FEATURE-SPECIFIC TESTING (4 files)
################################################################################

log "Generating feature-specific testing files..."

# 32. window_metrics.jsonl - Time-series data for window functions
cat > "$EXAMPLES_DIR/window_metrics.jsonl" <<'EOF'
{"timestamp":"2024-01-15T10:00:00Z","metric":"cpu","value":45.2,"host":"server1"}
{"timestamp":"2024-01-15T10:00:01Z","metric":"cpu","value":46.8,"host":"server1"}
{"timestamp":"2024-01-15T10:00:02Z","metric":"cpu","value":44.5,"host":"server1"}
{"timestamp":"2024-01-15T10:00:03Z","metric":"cpu","value":48.1,"host":"server1"}
{"timestamp":"2024-01-15T10:00:04Z","metric":"cpu","value":47.3,"host":"server1"}
{"timestamp":"2024-01-15T10:00:05Z","metric":"memory","value":2048,"host":"server1"}
{"timestamp":"2024-01-15T10:00:06Z","metric":"memory","value":2100,"host":"server1"}
{"timestamp":"2024-01-15T10:00:07Z","metric":"memory","value":2075,"host":"server1"}
{"timestamp":"2024-01-15T10:00:08Z","metric":"cpu","value":52.4,"host":"server1"}
{"timestamp":"2024-01-15T10:00:09Z","metric":"cpu","value":55.7,"host":"server1"}
{"timestamp":"2024-01-15T10:00:10Z","metric":"cpu","value":58.2,"host":"server1"}
{"timestamp":"2024-01-15T10:00:11Z","metric":"memory","value":2200,"host":"server1"}
{"timestamp":"2024-01-15T10:00:12Z","metric":"cpu","value":61.5,"host":"server1"}
{"timestamp":"2024-01-15T10:00:13Z","metric":"cpu","value":64.8,"host":"server1"}
{"timestamp":"2024-01-15T10:00:14Z","metric":"cpu","value":68.1,"host":"server1"}
{"timestamp":"2024-01-15T10:00:15Z","metric":"memory","value":2350,"host":"server1"}
{"timestamp":"2024-01-15T10:00:16Z","metric":"cpu","value":72.3,"host":"server1"}
{"timestamp":"2024-01-15T10:00:17Z","metric":"cpu","value":75.8,"host":"server1"}
{"timestamp":"2024-01-15T10:00:18Z","metric":"cpu","value":79.4,"host":"server1"}
{"timestamp":"2024-01-15T10:00:19Z","metric":"cpu","value":82.1,"host":"server1"}
{"timestamp":"2024-01-15T10:00:20Z","metric":"memory","value":2500,"host":"server1"}
{"timestamp":"2024-01-15T10:00:21Z","metric":"cpu","value":85.6,"host":"server1"}
{"timestamp":"2024-01-15T10:00:22Z","metric":"cpu","value":88.9,"host":"server1"}
{"timestamp":"2024-01-15T10:00:23Z","metric":"cpu","value":92.3,"host":"server1"}
{"timestamp":"2024-01-15T10:00:24Z","metric":"cpu","value":95.7,"host":"server1"}
{"timestamp":"2024-01-15T10:00:25Z","metric":"memory","value":2700,"host":"server1"}
{"timestamp":"2024-01-15T10:00:26Z","metric":"cpu","value":91.2,"host":"server1"}
{"timestamp":"2024-01-15T10:00:27Z","metric":"cpu","value":87.5,"host":"server1"}
{"timestamp":"2024-01-15T10:00:28Z","metric":"cpu","value":83.1,"host":"server1"}
{"timestamp":"2024-01-15T10:00:29Z","metric":"cpu","value":78.9,"host":"server1"}
EOF
success "Created window_metrics.jsonl (30 events, time-series)"

# 33. fan_out_batches.jsonl - Nested arrays for multi-level emit_each
cat > "$EXAMPLES_DIR/fan_out_batches.jsonl" <<'EOF'
{"batch_id":"batch_001","created":"2024-01-15T10:00:00Z","orders":[{"order_id":"ord_1","items":[{"sku":"A001","qty":2,"price":10.0},{"sku":"A002","qty":1,"price":20.0}]},{"order_id":"ord_2","items":[{"sku":"B001","qty":3,"price":15.0}]}]}
{"batch_id":"batch_002","created":"2024-01-15T10:01:00Z","orders":[{"order_id":"ord_3","items":[{"sku":"C001","qty":1,"price":50.0},{"sku":"C002","qty":2,"price":25.0},{"sku":"C003","qty":1,"price":30.0}]}]}
{"batch_id":"batch_003","created":"2024-01-15T10:02:00Z","orders":[{"order_id":"ord_4","items":[{"sku":"D001","qty":5,"price":8.0}]},{"order_id":"ord_5","items":[{"sku":"E001","qty":2,"price":12.0},{"sku":"E002","qty":3,"price":18.0}]},{"order_id":"ord_6","items":[{"sku":"F001","qty":1,"price":100.0}]}]}
{"batch_id":"batch_004","created":"2024-01-15T10:03:00Z","shipments":[{"shipment_id":"ship_1","packages":[{"tracking":"TRK001","weight":2.5},{"tracking":"TRK002","weight":3.0}]},{"shipment_id":"ship_2","packages":[{"tracking":"TRK003","weight":1.5}]}]}
{"batch_id":"batch_005","created":"2024-01-15T10:04:00Z","events":[{"event_type":"login","users":[{"id":1,"name":"alice"},{"id":2,"name":"bob"}]},{"event_type":"purchase","users":[{"id":3,"name":"charlie"}]}]}
EOF
success "Created fan_out_batches.jsonl (5 batches, multi-level nesting)"

# 34. custom_timestamps.log - Non-standard timestamp formats requiring --ts-format
cat > "$EXAMPLES_DIR/custom_timestamps.log" <<'EOF'
01/15/2024 10:00:00.123 Event with MM/DD/YYYY format
15-Jan-2024 10:00:05.456 Event with DD-Mon-YYYY format
2024.01.15 10:00:10.789 Event with dots as date separators
10:00:15 2024-01-15 Time before date
Jan 15, 2024 at 10:00:20 Natural language style
2024-01-15T10:00:25 No seconds, just minutes
20240115100030 Compact format YYYYMMDDHHMMSS
15/01/24 10:00:35 Two-digit year
2024-015 10:00:40 Day of year format
[2024-01-15 10:00:45,123] Bracketed with milliseconds
2024-01-15 10:00:50 AM 12-hour format
Mon, 15 Jan 2024 10:00:55 +0000 RFC 2822 style
@1705315260 Unix timestamp (seconds since epoch)
2024-W03-1 10:01:05 ISO week date
15.01.2024 - 10:01:10 European format with separator
EOF
success "Created custom_timestamps.log (15 formats requiring --ts-format)"

# 35. sampling_hash.jsonl.gz - High-volume data for bucket() sampling demos (gzipped)
log "Generating sampling_hash.jsonl.gz (600 events)..."
{
    for i in {1..600}; do
        user_id=$((1000 + i))
        action=("login" "view" "click" "purchase" "logout")
        act=${action[$((i % 5))]}
        status_code=$((i % 10 < 8 ? 200 : (i % 10 == 8 ? 404 : 500)))
        timestamp="2024-01-15T$(printf "%02d" $((10 + i / 60))):$(printf "%02d" $((i % 60))):00Z"
        echo "{\"timestamp\":\"$timestamp\",\"user_id\":$user_id,\"action\":\"$act\",\"status\":$status_code,\"session\":\"sess_$((i % 50))\"}"
    done
} | gzip > "$EXAMPLES_DIR/sampling_hash.jsonl.gz"
success "Created sampling_hash.jsonl.gz (600 events for sampling, gzipped)"

################################################################################
# EXTREMELY CHALLENGING (2 files - show off what kelora can do!)
################################################################################

log "Generating extremely challenging files that defeat other tools..."

# 36. nightmare_mixed_formats.log - Different formats interleaved in same file
# This would require multiple passes with jq/awk or complex preprocessing
cat > "$EXAMPLES_DIR/nightmare_mixed_formats.log" <<'EOF'
2024-01-15 10:00:00 [INFO] Server starting
{"timestamp":"2024-01-15T10:00:01Z","level":"DEBUG","format":"json","message":"Connection pool initialized","connections":50}
timestamp=2024-01-15T10:00:02Z level=info format=logfmt msg="Cache layer ready" size=1024
<34>Jan 15 10:00:03 appserver syslog: Authentication module loaded
web_1    | 2024-01-15 10:00:04 [INFO] HTTP server listening on port 8080
{"timestamp":"2024-01-15T10:00:05Z","level":"WARN","nested":{"data":{"deeply":{"buried":{"value":"hard to extract with jq"}}}}}
2024-01-15 10:00:06 [ERROR] Database connection failed
timestamp=2024-01-15T10:00:07Z level=error err="connection timeout" retry=3 max_retries=5
<27>Jan 15 10:00:08 appserver nginx: GET /api/users 200 1234
{"timestamp":"2024-01-15T10:00:09Z","users":[{"id":1,"name":"alice"},{"id":2,"name":"bob"}],"action":"batch_process"}
db_1     | 2024-01-15 10:00:10 [WARN] Slow query detected: SELECT * FROM large_table
2024-01-15 10:00:11 [INFO] Background worker started
timestamp=2024-01-15T10:00:12Z level=debug component=scheduler next_run="2024-01-15T11:00:00Z"
{"timestamp":"2024-01-15T10:00:13Z","level":"INFO","message":"Metric snapshot","metrics":{"cpu":75.5,"memory":2048,"disk":85}}
<14>Jan 15 10:00:14 loadbalancer haproxy: Server backend1/web1 is UP
worker_1 | 2024-01-15 10:00:15 [INFO] Job queue processed: 150 items
2024-01-15 10:00:16 [CRITICAL] Out of memory - killing processes
timestamp=2024-01-15T10:00:17Z level=critical action=restart reason="oom_killer"
{"timestamp":"2024-01-15T10:00:18Z","event":"deployment","version":"v1.2.3","services":["web","worker","db"],"status":"success"}
<19>Jan 15 10:00:19 appserver myapp[1234]: Request processing complete
redis_1  | 2024-01-15 10:00:20 [INFO] Connected clients: 42
EOF
success "Created nightmare_mixed_formats.log (mixed JSON/logfmt/syslog/prefixed in one file)"

# 37. nightmare_deeply_nested_transform.jsonl - Requires complex transformations
# Each event needs: nested extraction, array processing, conditional logic, aggregation
# This would be nearly impossible with jq one-liners and very verbose with awk
cat > "$EXAMPLES_DIR/nightmare_deeply_nested_transform.jsonl" <<'EOF'
{"request_id":"req_001","timestamp":"2024-01-15T10:00:00Z","api":{"endpoint":"/graphql","queries":[{"operation":"getUsers","filters":{"status":"active","role":{"in":["admin","moderator"]}},"results":{"users":[{"id":1,"name":"alice","permissions":{"read":true,"write":true,"delete":false},"last_login":"2024-01-14T15:30:00Z"},{"id":2,"name":"bob","permissions":{"read":true,"write":false,"delete":false},"last_login":"2024-01-13T09:15:00Z"}],"total":2,"page":1}},{"operation":"getPosts","filters":{"published":true,"tags":["tech","security"]},"results":{"posts":[{"id":101,"title":"Security Best Practices","author_id":1,"tags":["security","authentication"],"metrics":{"views":1523,"likes":89,"comments":[{"user_id":3,"text":"Great post!","sentiment":"positive"},{"user_id":4,"text":"Needs more examples","sentiment":"neutral"}]}},{"id":102,"title":"Tech Trends 2024","author_id":2,"tags":["tech","future"],"metrics":{"views":2341,"likes":156,"comments":[{"user_id":5,"text":"Very insightful","sentiment":"positive"}]}}],"total":2}}]},"response":{"status":200,"duration_ms":245,"cached":false}}
{"request_id":"req_002","timestamp":"2024-01-15T10:00:05Z","api":{"endpoint":"/rest/v2/orders","queries":[{"operation":"listOrders","filters":{"customer":{"region":"us-west","tier":"premium"},"date_range":{"start":"2024-01-01","end":"2024-01-15"}},"results":{"orders":[{"order_id":"ord_501","customer":{"id":1001,"name":"Acme Corp","contacts":[{"type":"primary","email":"orders@acme.com"},{"type":"billing","email":"billing@acme.com"}]},"items":[{"sku":"PROD-A","quantity":50,"unit_price":99.99,"discounts":[{"type":"volume","percent":10},{"type":"loyalty","percent":5}],"final_price":4274.79},{"sku":"PROD-B","quantity":25,"unit_price":149.99,"discounts":[{"type":"volume","percent":10}],"final_price":3374.78}],"totals":{"subtotal":7649.57,"tax":612.36,"shipping":25.00,"grand_total":8286.93},"fulfillment":{"warehouse":"WH-001","status":"shipped","tracking":"TRK12345","estimated_delivery":"2024-01-18"}},{"order_id":"ord_502","customer":{"id":1002,"name":"TechStart Inc","contacts":[{"type":"primary","email":"team@techstart.io"}]},"items":[{"sku":"PROD-C","quantity":100,"unit_price":49.99,"discounts":[],"final_price":4999.00}],"totals":{"subtotal":4999.00,"tax":399.92,"shipping":0.00,"grand_total":5398.92},"fulfillment":{"warehouse":"WH-002","status":"processing","tracking":null,"estimated_delivery":"2024-01-20"}}],"summary":{"total_orders":2,"total_revenue":13685.85,"avg_order_value":6842.93}}}]},"response":{"status":200,"duration_ms":567,"cached":true}}
{"request_id":"req_003","timestamp":"2024-01-15T10:00:10Z","api":{"endpoint":"/analytics/dashboard","queries":[{"operation":"getMetrics","time_range":{"start":"2024-01-15T09:00:00Z","end":"2024-01-15T10:00:00Z","granularity":"5m"},"results":{"timeseries":[{"timestamp":"2024-01-15T09:00:00Z","metrics":{"requests":1523,"errors":12,"latency":{"p50":45,"p95":234,"p99":567},"status_codes":{"2xx":1489,"4xx":22,"5xx":12}}},{"timestamp":"2024-01-15T09:05:00Z","metrics":{"requests":1687,"errors":8,"latency":{"p50":42,"p95":198,"p99":445},"status_codes":{"2xx":1665,"4xx":14,"5xx":8}}},{"timestamp":"2024-01-15T09:10:00Z","metrics":{"requests":1834,"errors":15,"latency":{"p50":48,"p95":267,"p99":623},"status_codes":{"2xx":1801,"4xx":18,"5xx":15}}}],"aggregates":{"total_requests":5044,"total_errors":35,"error_rate":0.69,"avg_latency":45,"peak_requests_per_min":367},"top_endpoints":[{"path":"/api/users","count":1234,"avg_latency":34},{"path":"/api/posts","count":987,"avg_latency":56},{"path":"/api/comments","count":654,"avg_latency":23}]}}]},"response":{"status":200,"duration_ms":1234,"cached":false}}
{"request_id":"req_004","timestamp":"2024-01-15T10:00:15Z","api":{"endpoint":"/ml/predictions","queries":[{"operation":"batchPredict","model":{"name":"fraud_detection","version":"v2.3.1","metadata":{"trained_on":"2024-01-10","accuracy":0.94,"features":["transaction_amount","user_history","device_fingerprint","location","time_of_day"]}},"inputs":[{"transaction_id":"txn_001","amount":1523.50,"user":{"id":5001,"history":{"total_transactions":156,"avg_amount":234.56,"fraud_reports":0}},"context":{"device":"iPhone","location":{"country":"US","city":"Portland","lat":45.5231,"lon":-122.6765},"time":"2024-01-15T09:45:00Z"}},{"transaction_id":"txn_002","amount":9999.99,"user":{"id":5002,"history":{"total_transactions":3,"avg_amount":45.67,"fraud_reports":0}},"context":{"device":"Unknown","location":{"country":"NG","city":"Lagos","lat":6.5244,"lon":3.3792},"time":"2024-01-15T09:50:00Z"}}],"predictions":[{"transaction_id":"txn_001","fraud_score":0.05,"decision":"approve","confidence":0.98,"factors":{"positive":["established_user","normal_location","typical_amount"],"negative":[]}},{"transaction_id":"txn_002","fraud_score":0.87,"decision":"review","confidence":0.91,"factors":{"positive":[],"negative":["new_user","unusual_location","high_amount","unknown_device"]}}]}]},"response":{"status":200,"duration_ms":892,"cached":false}}
{"request_id":"req_005","timestamp":"2024-01-15T10:00:20Z","api":{"endpoint":"/search/advanced","queries":[{"operation":"facetedSearch","query":"kubernetes security","filters":{"content_type":{"in":["article","video","tutorial"]},"publication_date":{"after":"2023-01-01"},"difficulty":{"in":["intermediate","advanced"]}},"facets":{"tags":{"security":234,"kubernetes":456,"devops":189,"containers":345},"authors":{"alice":12,"bob":8,"charlie":15},"years":{"2023":89,"2024":156}},"results":{"items":[{"id":"doc_001","title":"Kubernetes Security Best Practices","type":"article","author":"alice","tags":["kubernetes","security","rbac"],"metadata":{"published":"2024-01-10","updated":"2024-01-12","word_count":2345,"read_time_minutes":12},"relevance_score":0.95},{"id":"doc_002","title":"Securing Container Workloads","type":"video","author":"bob","tags":["security","containers","kubernetes"],"metadata":{"published":"2023-11-15","duration_seconds":1820,"views":5432},"relevance_score":0.89},{"id":"doc_003","title":"Advanced RBAC Configuration","type":"tutorial","author":"charlie","tags":["kubernetes","security","rbac","authorization"],"metadata":{"published":"2024-01-05","steps":15,"difficulty":"advanced","completion_rate":0.67},"relevance_score":0.87}],"pagination":{"total":245,"page":1,"per_page":3,"total_pages":82}}}]},"response":{"status":200,"duration_ms":178,"cached":false}}
EOF
success "Created nightmare_deeply_nested_transform.jsonl (deeply nested, requires multi-step transformations)"

echo ""
success "All 37 example files generated successfully in $EXAMPLES_DIR/"
echo ""
log "Summary:"
echo "  Basic formats:        8 files (JSON, CSV, TSV, logfmt, syslog, combined, CEF, line)"
echo "  Advanced formats:     6 files (cols, typed CSV, prefix extraction, key-value)"
echo "  Multiline scenarios:  5 files (stacktraces, JSON arrays, continuation, boundaries, indent)"
echo "  Complex real-world:   5 files (large dataset, nested JSON, arrays, security, timezones)"
echo "  Error handling:       7 files (malformed JSON, type errors, empty lines, ragged CSV, etc.)"
echo "  Feature-specific:     4 files (window functions, fan-out, custom timestamps, sampling)"
echo "  Nightmare mode:       2 files (mixed formats in one file, deeply nested transformations)"
echo ""
log "Run kelora on these files to test and demonstrate features!"
