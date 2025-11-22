# Configuration and Reusability

Learn how to create reusable workflows with aliases, configuration files, and external scripts. This tutorial shows you how to avoid repeating yourself and share common patterns with your team.

## What You'll Learn

- Create and use aliases with `--save-alias` and `-a`
- Understand configuration file locations and precedence
- View and edit configuration with `--show-config` and `--edit-config`
- Write reusable functions with `-I/--include`
- Organize complex scripts with `-E/--exec-file`
- Decide when to use aliases vs includes vs exec-files
- Share workflows with your team

## Prerequisites

- [Basics: Input, Display & Filtering](basics.md) - Basic CLI usage
- [Introduction to Rhai Scripting](intro-to-rhai.md) - Basic scripting knowledge
- **Time:** ~20 minutes

## Sample Data

This tutorial uses:

- `examples/simple_json.jsonl` - Application logs
- Example configuration files we'll create

---

## Part 1: Understanding Configuration Files

### Configuration File Locations

Kelora searches for configuration in two places (in order):

1. **Project config** - `.kelora.ini` in current directory (or parent directories)
2. **User config** - `~/.config/kelora/kelora.ini` (Unix) or `%APPDATA%\kelora\kelora.ini` (Windows)

**Precedence:** Project config overrides user config. CLI flags override both.

### View Current Configuration

=== "Command"

    ```bash
    kelora --show-config
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora --show-config
    ```

If no config exists, you'll see example templates and search paths.

---

## Part 2: Creating Your First Alias

Aliases let you save commonly-used command patterns.

### Save an Alias

Let's create an alias for finding errors:

=== "Command"

    ```bash
    kelora -j --levels error --keys timestamp,service,message --save-alias errors
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j --levels error --keys timestamp,service,message --save-alias errors
    ```

This saves the alias to your **user config file**.

### Use the Alias

Now you can use it with `-a`:

=== "Command"

    ```bash
    kelora -a errors examples/simple_json.jsonl
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -a errors examples/simple_json.jsonl
    ```

**What happened:** `kelora -a errors` expanded to `kelora -j --levels error --keys timestamp,service,message`.

---

## Part 3: Configuration File Structure

Configuration files use INI format with two sections:

```ini
# Root-level defaults applied to every command
defaults = -f auto --stats

# Named aliases invoked with -a/--alias
[aliases]
errors = -j --levels error --keys timestamp,service,message
slow = -j --filter 'e.duration_ms > 1000' --keys service,duration_ms,message
problems = -j --levels error,warn,critical
```

### Edit Your Configuration

=== "Command"

    ```bash
    kelora --edit-config
    ```

This opens your active config file in `$EDITOR` (vi/vim/nano on Unix, notepad on Windows).

---

## Part 4: Project vs User Configuration

### User Configuration (Personal Preferences)

Location: `~/.config/kelora/kelora.ini`

**Good for:**

- Personal formatting preferences
- Local development shortcuts
- Your own workflow patterns

**Example:**
```ini
defaults = --stats --no-emoji

[aliases]
myerrors = -j --levels error --exclude-keys password,token
quickcheck = -j -F none --stats
```

### Project Configuration (Team Shared)

Location: `.kelora.ini` in project root

**Good for:**

- Team-wide conventions
- Project-specific formats
- Shared analysis patterns

**Example:**
```ini
# Commit this to version control
defaults = -f json

[aliases]
api-errors = --levels error --filter 'e.service == "api"'
slow-db = --filter 'e.service == "database" && e.duration_ms > 100'
alerts = --levels critical,error --exec 'track_count(e.service)'
```

**Pro tip:** Commit `.kelora.ini` to your repository so the whole team uses the same patterns!

---

## Part 5: Advanced Aliases

### Chaining Multiple Aliases

Aliases can reference other aliases:

```ini
[aliases]
json = -f json
errors = --levels error
json-errors = -a json -a errors
```

=== "Command"

    ```bash
    # Create test config
    echo "[aliases]
    json = -f json
    errors = --levels error
    json-errors = -a json -a errors" > /tmp/test.ini

    kelora --config-file /tmp/test.ini -a json-errors examples/simple_json.jsonl --take 2
    rm /tmp/test.ini
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    echo "[aliases]
    json = -f json
    errors = --levels error
    json-errors = -a json -a errors" > /tmp/test.ini

    kelora --config-file /tmp/test.ini -a json-errors examples/simple_json.jsonl --take 2
    rm /tmp/test.ini
    ```

**Limit:** Aliases can reference up to 10 levels deep (prevents infinite loops).

### Complex Aliases with Scripting

You can put full pipelines in aliases:

```ini
[aliases]
analyze-api = -j --filter 'e.service == "api"' \
              --exec 'track_count(e.level)' \
              --exec 'track_sum("total_duration", e.duration_ms)' \
              --metrics -F none
```

**Tip:** Use backslash `\` for line continuation in INI files.

---

## Part 6: Reusable Functions with --include

For complex logic, create reusable Rhai function libraries.

### Create a Helper Library

We've created `examples/helpers.rhai` with useful functions:

=== "View File"

    ```bash
    cat examples/helpers.rhai
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    cat examples/helpers.rhai
    ```

### Use the Library

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        -I examples/helpers.rhai \
        --exec 'e.is_problem = is_problem(e)' \
        --filter 'e.is_problem' \
        -k service,level,is_problem,message
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        -I examples/helpers.rhai \
        --exec 'e.is_problem = is_problem(e)' \
        --filter 'e.is_problem' \
        -k service,level,is_problem,message
    ```

**How it works:**

1. `-I examples/helpers.rhai` loads the function definitions
2. Functions become available in all `--exec` and `--filter` stages
3. Call them like any built-in function

### Multiple Include Files

You can load multiple libraries:

```bash
kelora -j app.log \
    -I lib/validators.rhai \
    -I lib/transforms.rhai \
    -I lib/metrics.rhai \
    --exec 'e.valid = validate_event(e)' \
    --filter 'e.valid'
```

---

## Part 7: Complex Scripts with --exec-file

For longer transformations, put the entire script in a file.

### Create a Transformation Script

We've created `examples/enrich_events.rhai`:

=== "View File"

    ```bash
    cat examples/enrich_events.rhai
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    cat examples/enrich_events.rhai
    ```

### Use the Script

=== "Command"

    ```bash
    kelora -j examples/simple_json.jsonl \
        -E examples/enrich_events.rhai \
        -k date,time,severity,speed,service,message \
        --take 5
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora -j examples/simple_json.jsonl \
        -E examples/enrich_events.rhai \
        -k date,time,severity,speed,service,message \
        --take 5
    ```

**Note:** `-E` runs the script contents in the **exec stage** (once per event).

---

## Part 8: When to Use What

### Decision Matrix

| Need | Use | Why |
|------|-----|-----|
| **Common flag combinations** | Alias | Quick, sharable, version-controlled |
| **Reusable functions** | `--include` | Define once, use in any `--exec`/`--filter` |
| **Multi-step transformations** | `--exec-file` | Keep complex logic organized in files |
| **Personal shortcuts** | User config alias | Quick access to your patterns |
| **Team conventions** | Project config alias | Everyone uses same workflows |
| **One-off commands** | Plain CLI flags | No need to save |

### Pattern Examples

#### Pattern 1: Personal Shortcut (User Alias)
```bash
# Save to user config
kelora -j --keys timestamp,message -b --save-alias quick

# Use anywhere
kelora -a quick any-file.log
```

#### Pattern 2: Team Convention (Project Alias)
```bash
# In .kelora.ini (commit to repo)
[aliases]
production-errors = -j --levels error,critical \
                    --filter 'e.environment == "production"' \
                    --exec 'track_count(e.service)'

# Anyone on team can run
kelora -a production-errors logs/*.jsonl --metrics
```

#### Pattern 3: Reusable Logic (Include File)
```bash
# lib/validation.rhai
fn is_valid_email(text) {
    text.contains("@") && text.contains(".")
}

# Use in pipeline
kelora -j app.log \
    -I lib/validation.rhai \
    --filter 'is_valid_email(e.email)'
```

#### Pattern 4: Complex Pipeline (Exec File)
```bash
# transforms/normalize.rhai
// Normalize all fields
e.level = e.level.to_upper();
e.service = e.service.to_lower();

if e.has("duration_ms") {
    e.duration_s = e.duration_ms / 1000;
}

track_count(e.service + ":" + e.level);

# Use in pipeline
kelora -j app.log -E transforms/normalize.rhai --metrics
```

---

## Part 9: Real-World Workflow

Let's build a complete reusable workflow.

### Step 1: Create Project Structure

```bash
mkdir -p myproject/scripts
cd myproject
```

### Step 2: Create Shared Functions

Create `scripts/helpers.rhai`:
```rhai
fn critical_event(e) {
    e.level == "CRITICAL" ||
    (e.level == "ERROR" && e.service == "database")
}

fn add_alert_tag(e) {
    if critical_event(e) {
        e.alert = true;
        e.priority = "P1";
    } else if e.level == "ERROR" {
        e.alert = true;
        e.priority = "P2";
    }
}
```

### Step 3: Create Transformation Script

Create `scripts/enrich.rhai`:
```rhai
// Add computed fields
if e.has("duration_ms") {
    e.duration_s = e.duration_ms / 1000;
    e.slow = e.duration_ms > 1000;
}

// Classify severity
if e.has("level") {
    e.severity = if e.level == "CRITICAL" || e.level == "ERROR" {
        "high"
    } else if e.level == "WARN" {
        "medium"
    } else {
        "low"
    };
}
```

### Step 3: Create Project Config

Create `.kelora.ini`:
```ini
defaults = -f json --stats

[aliases]
errors = --levels error,critical -I scripts/helpers.rhai
enrich = -I scripts/helpers.rhai -E scripts/enrich.rhai
alerts = -a errors --exec 'add_alert_tag(e)' --filter 'e.alert'
analyze = -a enrich --exec 'track_count(e.severity)' --metrics
```

### Step 4: Use the Workflow

```bash
# Quick error check
kelora -a errors app.log

# Full enrichment
kelora -a enrich app.log --take 10

# Generate alerts
kelora -a alerts app.log -F json -o alerts.json

# Analyze severity distribution
kelora -a analyze app.log
```

### Step 5: Share with Team

```bash
git add .kelora.ini scripts/
git commit -m "Add log analysis workflows"
git push
```

Now your team can clone and run the same patterns!

---

## Part 10: Configuration Best Practices

### ✅ Do

- **Commit project `.kelora.ini`** - Share team conventions
- **Keep aliases focused** - One purpose per alias
- **Document in README** - Explain what each alias does
- **Use includes for functions** - Reusable logic in files
- **Version control scripts** - Track changes to transforms
- **Test aliases** - Verify they work before committing

### ❌ Don't

- **Don't put secrets in config** - Use environment variables instead
- **Don't make aliases too complex** - Use exec-files for long scripts
- **Don't override critical flags** - Be careful with defaults
- **Don't chain too deeply** - Keep alias references simple
- **Don't commit personal preferences** - Use user config for those

---

## Part 11: Troubleshooting

### View Expanded Command

See what aliases expand to:

```bash
kelora -a myalias app.log --verbose
```

### Ignore Configuration

Test without config:

```bash
kelora --ignore-config -j app.log --levels error
```

### Use Specific Config File

Test project config without affecting user config:

```bash
kelora --config-file .kelora.ini --show-config
```

### Check Include File Errors

```bash
kelora -I myfile.rhai app.log --verbose
# Shows any syntax errors in the include file
```

---

## Part 12: Advanced Techniques

### Conditional Defaults

You can't have conditionals in INI, but you can create multiple configs:

```bash
# .kelora.dev.ini (development)
defaults = -f json --stats --verbose

# .kelora.prod.ini (production)
defaults = -f json -q

# Use specific config
kelora --config-file .kelora.dev.ini app.log
```

### Environment-Specific Aliases

```bash
[aliases]
dev = -f json --stats --verbose
prod = -f json -q --strict
staging = -f json --stats

# Use based on environment
kelora -a $ENV app.log  # where ENV=dev|prod|staging
```

### Combining with Shell Scripts

Create `analyze.sh`:
```bash
#!/bin/bash
# Production log analyzer

set -e

LOG_FILE="$1"
OUTPUT_DIR="results"

mkdir -p "$OUTPUT_DIR"

# Run multiple analyses
kelora -a errors "$LOG_FILE" -o "$OUTPUT_DIR/errors.json"
kelora -a slow "$LOG_FILE" -o "$OUTPUT_DIR/slow.json"
kelora -a analyze "$LOG_FILE" > "$OUTPUT_DIR/metrics.txt"

echo "Analysis complete. Results in $OUTPUT_DIR/"
```

---

## Summary

You've learned:

- ✅ Create aliases with `--save-alias` for reusable commands
- ✅ Use aliases with `-a` to avoid repetition
- ✅ Understand project vs user configuration
- ✅ View config with `--show-config` and edit with `--edit-config`
- ✅ Write reusable functions with `-I/--include`
- ✅ Organize complex scripts with `-E/--exec-file`
- ✅ Choose the right tool for each use case
- ✅ Share workflows with your team via version control
- ✅ Best practices for configuration management

## Practice Exercises

### Exercise 1: Create a Personal Alias

Create an alias for your most common log viewing pattern:

<details>
<summary>Solution</summary>

```bash
# Example: compact error view
kelora -j --levels error -k timestamp,service,message -b --save-alias myerrors

# Use it
kelora -a myerrors app.log
```
</details>

### Exercise 2: Build a Helper Library

Create `myhelpers.rhai` with a function to check if a status code is an error:

<details>
<summary>Solution</summary>

```rhai
// myhelpers.rhai
fn is_error_status(status) {
    status >= 400 && status < 600
}

// Use it
kelora -j app.log \
    -I myhelpers.rhai \
    --filter 'e.has("status") && is_error_status(e.status)'
```
</details>

### Exercise 3: Create a Project Workflow

Set up a `.kelora.ini` for a web application with aliases for:

- API errors
- Slow requests
- Traffic analysis

<details>
<summary>Solution</summary>

```ini
defaults = -f combined

[aliases]
api-errors = --filter 'e.status >= 400' --keys ip,status,method,path
slow = --filter 'e.request_time.to_float() > 1.0' --keys ip,request_time,path
traffic = --exec 'track_count(e.method); track_count(e.status.to_string())' --metrics -F none
```
</details>

---

## Next Steps

Now that you can create reusable workflows, continue to:

- **[Advanced Scripting](advanced-scripting.md)** - Advanced transformation patterns
- **[Metrics and Tracking](metrics-and-tracking.md)** - Build analytics into your aliases
- **[Working with Time](working-with-time.md)** - Time-based filtering in aliases

**Related guides:**

- [Concepts: Configuration System](../concepts/configuration-system.md) - Deep dive into config precedence
- [How-To: Build a Service Health Snapshot](../how-to/monitor-application-health.md) - Real-world alias examples
- [CLI Reference: Configuration Options](../reference/cli-reference.md#configuration-options) - All config flags
