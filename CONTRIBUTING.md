# Contributing to Kelora

Thanks for your interest in Kelora! This is a solo project where I'm exploring AI-assisted development workflows, so I'm keeping the implementation work to myself. However, there are valuable ways you can contribute.

## What I'm Looking For

### 🔹 Real-World Log Files

The best contribution you can make is sharing interesting log files and formats:

- Anonymized production logs with tricky formats
- Multi-line logs (stack traces, JSON arrays, etc.)
- Custom application log formats
- Logs that other tools struggle with
- Large-scale examples (GBs of compressed logs)

**How to share**: Open an issue with:
- Sample log lines (anonymized)
- Brief description of the format
- What you're trying to extract or analyze
- Current pain points with existing tools

### 🔹 Use Cases and Scenarios

Describe log analysis problems you face:

- "I need to correlate errors across microservices"
- "I want to detect API endpoints with increasing latency"
- "I need to mask PII before sharing logs with support"

Real problems help prioritize features and improve documentation.

### 🔹 Bug Reports

Found a bug? Please report it:

1. Check existing issues first
2. Include Kelora version (`kelora --version`)
3. Provide minimal reproduction steps
4. Share sample input (if possible)
5. Describe expected vs actual behavior

### 🔹 Documentation Improvements

Spot a typo, confusing explanation, or missing example?

- Open an issue describing the problem
- Suggest improved wording
- Point out unclear sections

I'll incorporate good suggestions.

### 🔹 Benchmark Results

Run the comparison benchmarks on your hardware:

```bash
just bench-compare
```

Submit results with:
- System specs (CPU, RAM, OS)
- Tool versions
- Raw benchmark output from `benchmarks/comparison_results/`

Helps build a performance picture across different systems.

## What I'm Not Looking For

### ❌ Code Pull Requests

I'm keeping implementation work in-house as part of the development experiment. Code PRs won't be accepted, but feature requests and bug reports are welcome.

### ❌ Architectural Debates

The core design decisions (Rust, Rhai, streaming architecture) are settled. I'm focused on refinement rather than reimagining.

## Communication

- **Issues**: For bugs, features, and questions
- **Discussions**: For open-ended topics and use cases
- **Email**: security@dirk-loss.de for security issues only

## Response Time

This is a spare-time project maintained by one person. Expect responses within a few days to a week. Critical security issues get priority.

## Code of Conduct

Be respectful and constructive. That's it. We're all here to build useful tools and learn.

## License

By contributing use cases, log samples, or documentation suggestions, you agree they can be used under the MIT License like the rest of the project.

---

**Have an interesting log analysis challenge?** Open an issue - I'd love to hear about it!
